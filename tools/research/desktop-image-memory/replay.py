#!/usr/bin/env python3
"""Prepare and replay matched, non-race Windows image-memory variants."""
import argparse,base64,hashlib,json,os,re,shutil,subprocess,sys,tempfile,ntpath
from pathlib import Path

HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[2]
COMMON=HERE.parent/'windows-desktop-memory'
sys.path.insert(0,str(COMMON))
BASE='173d362036ae4a746878306915c85bcdf7468751'
IMPLEMENTATION='ea91a8f6693860dd3af2859222ca1f3c4b4ffe03'
PRODUCT=['app/desktop/internal/desktopui/'+n for n in ['live.go','live_details.go','live_images.go','live_image_requests.go']]
VARIANTS=['baseline','admission','streaming','combined']

def run(args,**kw):return subprocess.run(args,check=True,**kw)
def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def quote(s):
    value=str(s);slash=chr(92)
    if re.match(r'^[A-Za-z]:',value):value=ntpath.normpath(value)
    elif value.startswith(slash*2):value=ntpath.normpath(slash*2+value.lstrip(slash))
    return "'"+value.replace("'","''")+"'"
def ps(vm,source):
    source="$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';[Console]::OutputEncoding=[Text.UTF8Encoding]::new();\n"+source
    result=subprocess.check_output(['prlctl','exec',vm,'--current-user','powershell.exe','-ExecutionPolicy','Bypass','-NoProfile','-EncodedCommand',base64.b64encode(source.encode('utf-16le')).decode()],text=True,stderr=subprocess.STDOUT,timeout=300)
    return '\n'.join(line for line in result.splitlines() if not line.startswith('request_id='))

def prepare(stage):
    if stage.exists():raise RuntimeError('Stage must be fresh')
    stage.mkdir(parents=True)
    manifest={'schema':1,'base_sha':BASE,'implementation_sha':subprocess.check_output(['git','rev-parse',IMPLEMENTATION],cwd=ROOT,text=True).strip(),'origin':subprocess.check_output(['git','remote','get-url','origin'],cwd=ROOT,text=True).strip(),'go':subprocess.check_output(['go','version'],text=True).strip(),'race':False,'patch_sha256':{p.name:digest(p) for p in HERE.glob('*.patch')},'variants':{}}
    shutil.copytree(COMMON,stage/'scripts',ignore=shutil.ignore_patterns('__pycache__'))
    shutil.copyfile(HERE/'measure.ps1',stage/'scripts/measure.ps1')
    shutil.copyfile(HERE/'cache_probe.go.txt',stage/'scripts/cache_probe.go.txt')
    manifest['diagnostic_source_sha256']={'cache_probe.go.txt':digest(HERE/'cache_probe.go.txt'),'diagnostic_init.go.txt':digest(COMMON/'diagnostic_init.go.txt'),'view_hook':'s.d.view(c); memoryResearchSeen(s)'}
    with tempfile.TemporaryDirectory(prefix='unic-image-memory-source-') as scratch:
        for variant in VARIANTS:
            src=Path(scratch)/variant;src.mkdir()
            archive=subprocess.Popen(['git','archive',BASE],cwd=ROOT,stdout=subprocess.PIPE)
            run(['tar','-x','-C',str(src)],stdin=archive.stdout);archive.stdout.close()
            if archive.wait():raise RuntimeError('Archive failed')
            for patch in (['admission'] if variant=='admission' else ['streaming'] if variant=='streaming' else ['admission','streaming'] if variant=='combined' else []):
                run(['patch','--no-backup-if-mismatch','-p1','-i',str(HERE/(patch+'.patch'))],cwd=src)
            source_hash={p:digest(src/p) for p in PRODUCT if (src/p).exists()}
            if variant=='combined':
                for p,h in source_hash.items():
                    expected=subprocess.check_output(['git','show',IMPLEMENTATION+':'+p],cwd=ROOT)
                    if hashlib.sha256(expected).hexdigest()!=h:raise RuntimeError('Combined source differs from approved implementation: '+p)
            for folder in ['app/desktop/cmd/unic-desktop','app/service/cmd/unicd']:
                shutil.copyfile(COMMON/'diagnostic_init.go.txt',src/folder/'memory_research.go')
            shutil.copyfile(HERE/'cache_probe.go.txt',src/'app/desktop/internal/desktopui/memory_research.go')
            live=src/PRODUCT[0];text=live.read_text();old='func (s *Desktop) View(c *ui.Context) { s.d.view(c) }'
            if text.count(old)!=1:raise RuntimeError('View seam changed')
            live.write_text(text.replace(old,'func (s *Desktop) View(c *ui.Context) { s.d.view(c); memoryResearchSeen(s) }'))
            manifest['variants'][variant]={'product_sha256':source_hash,'binaries':{}}
            for arch in ['arm64','amd64']:
                dest=stage/(variant+'-'+arch);dest.mkdir()
                env=dict(os.environ,GOOS='windows',GOARCH=arch,CGO_ENABLED='0')
                run(['go','build','-trimpath','-tags','desktop_e2e','-ldflags=-s -w -H windowsgui','-o',str(dest/'unic-desktop.exe'),'./app/desktop/cmd/unic-desktop'],cwd=src,env=env)
                for name in ['unic','unicd']:
                    if variant=='baseline':run(['go','build','-trimpath','-ldflags=-s -w','-o',str(dest/(name+'.exe')),'./app/service/cmd/'+name],cwd=src,env=env)
                    else:shutil.copyfile(stage/('baseline-'+arch)/(name+'.exe'),dest/(name+'.exe'))
                manifest['variants'][variant]['binaries'][arch]={p.name:digest(p) for p in dest.glob('*.exe')}
            if variant=='baseline':
                probe=src/'tools/memory-service-probe';probe.mkdir(parents=True);shutil.copyfile(COMMON/'service_probe.go.txt',probe/'main.go')
                env=dict(os.environ,GOOS='windows',GOARCH='arm64',CGO_ENABLED='0')
                run(['go','build','-trimpath','-o',str(stage/'service-probe.exe'),'./tools/memory-service-probe'],cwd=src,env=env)
                run(['go','build','-o',str(stage/'mygo-automation.exe'),'github.com/egoist/mygo/plugins/automation/cmd/mygo-automation'],cwd=src,env=env)
    run(['python3',str(COMMON/'generate-fixtures.py'),str(stage/'scripts/fixtures')])
    if json.loads((stage/'scripts/fixtures/manifest.json').read_text())!=json.loads((ROOT/'docs/exec/windows-desktop-memory/fixture-manifest.json').read_text()):raise RuntimeError('Fixture versions differ')
    manifest['stage_sha256']={str(p.relative_to(stage)):digest(p) for p in sorted(stage.rglob('*')) if p.is_file()}
    (stage/'build-identity.json').write_text(json.dumps(manifest,indent=2)+'\n')
    print('PREPARED '+str(stage),flush=True)

def execute(stage,output,vm,repetitions,datasets,architectures,variants):
    if output.exists():raise RuntimeError('Output must be fresh')
    if not stage.is_relative_to(Path.home()):raise RuntimeError('Stage must be under the shared home directory')
    manifest=json.loads((stage/'build-identity.json').read_text())
    for name,h in manifest['stage_sha256'].items():
        if digest(stage/name)!=h:raise RuntimeError('Prepared stage changed: '+name)
    info=subprocess.check_output(['prlctl','list','-i',vm],text=True)
    match=re.search(r'^State: (\S+)',info,re.M);state=match.group(1) if match else None
    if state not in ['running','suspended']:raise RuntimeError('Expected running/suspended VM')
    guest='C:\\uim-db66';share='\\\\Mac\\Home\\'+str(stage.relative_to(Path.home())).replace('/','\\')
    output.mkdir(parents=True);shutil.copyfile(stage/'build-identity.json',output/'build-identity.json')
    (output/'vm-identity.txt').write_text(info)
    resumed=False;created=False;success=False;records=[]
    try:
        if state=='suspended':run(['prlctl','resume',vm]);resumed=True
        print(ps(vm,"if(Get-CimInstance Win32_Process | Where-Object {$_.Name -match '^unic(-desktop|d)?\\.exe$'}){throw 'Another task owns application processes'}\nif(Test-Path "+quote(guest)+"){throw 'Guest task directory exists'}"),flush=True)
        created=True
        print(ps(vm,'Copy-Item '+quote(share)+' '+quote(guest)+' -Recurse'),flush=True)
        (stage/'results').mkdir()
        guest_info=ps(vm,"$os=Get-CimInstance Win32_OperatingSystem;$cpu=Get-CimInstance Win32_Processor | Select-Object -First 1;$sys=Get-CimInstance Win32_ComputerSystem;[pscustomobject]@{caption=$os.Caption;version=$os.Version;build=$os.BuildNumber;osArchitecture=$os.OSArchitecture;processorArchitecture=$cpu.Architecture;logicalProcessors=$sys.NumberOfLogicalProcessors;physicalMemoryBytes=$sys.TotalPhysicalMemory;pageBytes=[Environment]::SystemPageSize;consoleSession=(Get-Process -Id $PID).SessionId} | ConvertTo-Json")
        (output/'guest-identity.json').write_text(guest_info)

        for rep in range(1,repetitions+1):
            for dataset in datasets:
                for arch in architectures:
                    selected=[v for v in variants if dataset=='images' or v in ['baseline','combined']]
                    if arch=='amd64':selected=[v for v in selected if dataset=='images' and v in ['baseline','combined']]
                    if rep%2==0:selected=selected[::-1]
                    for variant in selected:
                        name=f'{variant}-{arch}-{dataset}-{rep}';print('MEASURING '+name,flush=True)
                        text='& '+quote(guest+'\\scripts\\measure.ps1')+' -BinaryDir '+quote(guest+'\\'+variant+'-'+arch)+' -RunRoot '+quote(guest+'\\'+name)+' -Dataset '+dataset+' -Repetition '+str(rep)+' -Diagnostic\n$dst='+quote(share+'\\results\\'+name)+";New-Item $dst -ItemType Directory | Out-Null\nCopy-Item "+quote(guest+'\\'+name+'\\*.json*')+" $dst\nCopy-Item "+quote(guest+'\\'+name+'\\runtime*')+" $dst\nCopy-Item "+quote(guest+'\\'+name+'\\selected-native.png')+' $dst\nif(Test-Path '+quote(guest+'\\'+name+'\\selected-after-native.png')+') {Copy-Item '+quote(guest+'\\'+name+'\\selected-after-native.png')+' $dst}'
                        print(ps(vm,text),flush=True)
                        run(['python3',str(HERE/'summarize.py'),str(stage/'results'),'--output',str(output)])
                        if (stage/'STOP_AFTER_RUN').exists():raise RuntimeError('Requested stop after completed run')
        run(['python3',str(HERE/'analyze.py'),str(output/'measurements.json'),'--output',str(output/'comparison.json')])
        success=True
    except BaseException as exc:
        failure={'type':type(exc).__name__,'message':str(exc),'output':getattr(exc,'output','')}
        (output/'failure.json').write_text(json.dumps(failure,default=str,indent=2)+'\n')
        raise
    finally:
        cleanup_errors=[]
        if created:
            cleanup_script=r"""
$root=ROOT_LITERAL
# Stop only this runner's sampler before stopping its programs. Otherwise a
# sampler interrupted during seeding could spawn the next program during cleanup.
$owners=@(Get-CimInstance Win32_Process | Where-Object {$_.Name -eq 'powershell.exe' -and $_.ProcessId -ne $PID -and $_.CommandLine -match '-EncodedCommand\s+(\S+)'} | Where-Object {
 $encoded=[regex]::Match($_.CommandLine,'-EncodedCommand\s+(\S+)').Groups[1].Value
 try{[Text.Encoding]::Unicode.GetString([Convert]::FromBase64String($encoded.Trim('"'))).Contains("& '"+$root+"\scripts\measure.ps1'")}catch{$false}
})
foreach($p in $owners){Stop-Process -Id $p.ProcessId -Force}
for($attempt=0;$attempt -lt 5;$attempt++){
 $owned=@(Get-CimInstance Win32_Process | Where-Object {$_.ExecutablePath -and $_.ExecutablePath.StartsWith($root+'\',[StringComparison]::OrdinalIgnoreCase)})
 foreach($p in $owned){$process=Get-Process -Id $p.ProcessId -ErrorAction SilentlyContinue;if($process){Stop-Process -Id $p.ProcessId -Force;$process.WaitForExit(10000)|Out-Null}}
 Start-Sleep -Milliseconds 300
}
if(Test-Path $root){Remove-Item $root -Recurse -Force}
if(Test-Path $root){throw 'Guest cleanup failed'}
""".replace('ROOT_LITERAL',quote(guest))
            try:print(ps(vm,cleanup_script),flush=True)
            except Exception as exc:cleanup_errors.append('guest: '+str(exc))
        if resumed:
            try:run(['prlctl','suspend',vm])
            except Exception as exc:cleanup_errors.append('VM restore: '+str(exc))
        if not cleanup_errors:shutil.rmtree(stage)
        (output/'cleanup.json').write_text(json.dumps({'completed':success,'guest_task_removed':created and not cleanup_errors,'host_stage_removed':not stage.exists(),'vm_restored_to':state,'system_clipboard_read_or_written':False,'errors':cleanup_errors},indent=2)+'\n')
        with (output/'SHA256SUMS').open('w') as f:
            for path in sorted(output.iterdir()):
                if path.is_file() and path.name!='SHA256SUMS':f.write(digest(path)+'  '+path.name+'\n')
        if cleanup_errors:raise RuntimeError('Cleanup also failed; original failure preserved in failure.json: '+'; '.join(cleanup_errors))

if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('action',choices=['prepare','run','all']);p.add_argument('--stage',type=Path,required=True);p.add_argument('--output',type=Path);p.add_argument('--vm',default='Windows 11');p.add_argument('--repetitions',type=int,default=3);p.add_argument('--datasets',nargs='+',choices=['empty','text','images'],default=['empty','text','images']);p.add_argument('--architectures',nargs='+',choices=['arm64','amd64'],default=['arm64','amd64']);p.add_argument('--variants',nargs='+',choices=VARIANTS,default=VARIANTS);a=p.parse_args()
    if not 1<=a.repetitions<=3:raise SystemExit('Repetitions must be1..3')
    if a.action in ['run','all'] and not a.output:raise SystemExit('Output required')
    if a.action in ['prepare','all']:prepare(a.stage.resolve())
    if a.action in ['run','all']:
        execute(a.stage.resolve(),a.output.resolve(),a.vm,a.repetitions,a.datasets,a.architectures,a.variants)
