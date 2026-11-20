#!/usr/bin/env python3
"""Replay isolated Windows native measurements. No installer, personal profile or clipboard access."""
import argparse,base64,hashlib,json,os,re,shutil,subprocess,tempfile,uuid
from pathlib import Path
import summarize

BASE='173d362036ae4a746878306915c85bcdf7468751'
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--vm',default='Windows 11');p.add_argument('--source',default=BASE)
p.add_argument('--output',type=Path,required=True);p.add_argument('--repetitions',type=int,default=3)
p.add_argument('--quick-only',action='store_true')
p.add_argument('--detail-admission-control',action='store_true')
p.add_argument('--stream-neighbors',action='store_true')
p.add_argument('--explore',action='store_true');p.add_argument('--cycles',type=int,default=0)
p.add_argument('--datasets',nargs='+',choices=['empty','text','images'],default=['empty','text','images'])
p.add_argument('--architectures',nargs='+',choices=['amd64','arm64'],default=['amd64','arm64'])
a=p.parse_args();a.output=a.output.resolve()
if a.output.exists():raise SystemExit('Output must be fresh')
if not 0<=a.cycles<=10:raise SystemExit('Cycles must be 0..10')
if not 1<=a.repetitions<=3:raise SystemExit('Repetitions must be 1..3')
repo=Path(__file__).resolve().parents[3];scripts=Path(__file__).resolve().parent

def run(args,**kw):return subprocess.run(args,check=True,**kw)
def ps(src):
    src="$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';[Console]::OutputEncoding=[Text.UTF8Encoding]::new();\n"+src
    run(['prlctl','exec',a.vm,'--current-user','powershell.exe','-ExecutionPolicy','Bypass','-NoProfile','-EncodedCommand',base64.b64encode(src.encode('utf-16le')).decode()])
def sq(s):return "'"+str(s).replace("'","''")+"'"

actual=subprocess.check_output(['git','rev-parse',a.source+'^{commit}'],cwd=repo,text=True).strip()
info=subprocess.check_output(['prlctl','list','-i',a.vm],text=True)
match=re.search(r'^State: (\S+)',info,re.M);state=match.group(1) if match else None
if state not in ('running','suspended'):raise SystemExit('Replay requires a running or suspended existing VM')
token=uuid.uuid4().hex[:8];guest='C:\\um-'+token
stage=Path.home()/'.cache'/('unic-memory-replay-'+token);stage.mkdir(parents=True)
share='\\\\Mac\\Home\\'+str(stage.relative_to(Path.home())).replace('/','\\')
resumed=False;guest_created=False;a.output.mkdir(parents=True)
try:
    with tempfile.TemporaryDirectory(prefix='unic-memory-source-') as temp:
        src=Path(temp)
        archive=subprocess.Popen(['git','archive',actual],cwd=repo,stdout=subprocess.PIPE)
        run(['tar','-x','-C',str(src)],stdin=archive.stdout);archive.stdout.close()
        if archive.wait():raise RuntimeError('Source snapshot failed')
        for rel in ['app/desktop/cmd/unic-desktop','app/service/cmd/unicd']:
            shutil.copyfile(scripts/'diagnostic_init.go.txt',src/rel/'memory_research.go')
        shutil.copyfile(scripts/'cache_probe.go.txt',src/'app/desktop/internal/desktopui/memory_research.go')
        live=src/'app/desktop/internal/desktopui/live.go';text=live.read_text()
        old='func (s *Desktop) View(c *ui.Context) { s.d.view(c) }'
        if text.count(old)!=1:raise RuntimeError('Source seam changed; review the diagnostic patch')
        live.write_text(text.replace(old,'func (s *Desktop) View(c *ui.Context) { memoryResearchSeen(s); s.d.view(c) }'))
        if a.detail_admission_control:
            run(['patch','-p1','-i',str(scripts/'detail-admission-control.patch')],cwd=src)
        if a.stream_neighbors:
            run(['patch','-p1','-i',str(scripts/'stream-neighbor-input.patch')],cwd=src)
        for arch in a.architectures:
            out=stage/('diag-'+arch);out.mkdir()
            env=dict(os.environ,GOOS='windows',GOARCH=arch,CGO_ENABLED='0')
            for program,path,tags in [('unic-desktop','./app/desktop/cmd/unic-desktop',['-tags','desktop_e2e']),('unicd','./app/service/cmd/unicd',[]),('unic','./app/service/cmd/unic',[])]:
                flags='-s -w'+(' -H windowsgui' if program=='unic-desktop' else '')
                run(['go','build','-trimpath',*tags,'-ldflags='+flags,'-o',str(out/(program+'.exe')),path],cwd=src,env=env)
        probe=src/'tools/memory-service-probe';probe.mkdir(parents=True)
        shutil.copyfile(scripts/'service_probe.go.txt',probe/'main.go')
        env=dict(os.environ,GOOS='windows',GOARCH='arm64',CGO_ENABLED='0')
        run(['go','build','-trimpath','-o',str(stage/'service-probe.exe'),'./tools/memory-service-probe'],cwd=src,env=env)
        run(['go','build','-o',str(stage/'mygo-automation.exe'),'github.com/egoist/mygo/plugins/automation/cmd/mygo-automation'],cwd=src,env=env)
        identity={'source_sha':actual,'detail_admission_control':a.detail_admission_control,'live_details_sha256':hashlib.sha256((src/'app/desktop/internal/desktopui/live_details.go').read_bytes()).hexdigest(),'stream_neighbors':a.stream_neighbors,'live_images_sha256':hashlib.sha256((src/'app/desktop/internal/desktopui/live_images.go').read_bytes()).hexdigest(),'view_hook_sha256':hashlib.sha256(live.read_bytes()).hexdigest(),'origin':subprocess.check_output(['git','remote','get-url','origin'],cwd=repo,text=True).strip(),'vm_initial_state':state,'vm_name':a.vm,'binary_sha256':{str(f.relative_to(stage)):hashlib.sha256(f.read_bytes()).hexdigest() for f in stage.glob('**/*.exe')},'diagnostic_source_sha256':{str(f.relative_to(src)):hashlib.sha256(f.read_bytes()).hexdigest() for f in src.glob('**/memory_research.go')}}
        (a.output/'replay-identity.json').write_text(json.dumps(identity,indent=2)+'\n')
    shutil.copytree(scripts,stage/'scripts',ignore=shutil.ignore_patterns('__pycache__'))
    run(['python3',str(scripts/'generate-fixtures.py'),str(stage/'scripts/fixtures')])
    expected=repo/'docs/exec/windows-desktop-memory/fixture-manifest.json'
    if expected.exists() and json.loads(expected.read_text())!=json.loads((stage/'scripts/fixtures/manifest.json').read_text()):
        raise RuntimeError('Fixture encoding differs from the recorded corpus; use the recorded Pillow/zlib versions')
    if state=='suspended':run(['prlctl','resume',a.vm]);resumed=True
    ps("if(Get-CimInstance Win32_Process | Where-Object {$_.Name -eq 'unic-desktop.exe' -or $_.Name -eq 'unicd.exe'}){throw 'Existing application/service; coordinate before replay'}\nif(Test-Path "+sq(guest)+"){throw 'Guest task path collision'}\nCopy-Item "+sq(share)+' '+sq(guest)+" -Recurse")
    guest_created=True
    (stage/'results').mkdir()
    for rep in range(1,a.repetitions+1):
        for dataset in a.datasets:
            for arch in (a.architectures if rep%2 else a.architectures[::-1]):
                name=f'matrix-{arch}-{dataset}-{rep}';print('Measuring '+name,flush=True)
                extra=(' -QuickOnly' if a.quick_only and dataset=='images' and rep==1 else '')+(' -Explore' if a.explore and dataset=='images' and rep==1 else '')+(' -Cycles '+str(a.cycles) if a.cycles and dataset=='images' and rep==1 else '')
                ps('& '+sq(guest+'\\scripts\\measure.ps1')+' -BinaryDir '+sq(guest+'\\diag-'+arch)+' -RunRoot '+sq(guest+'\\'+name)+' -Dataset '+dataset+' -Repetition '+str(rep)+' -Diagnostic'+extra+'\n$dst='+sq(share+'\\results\\'+name)+";New-Item $dst -ItemType Directory | Out-Null\nCopy-Item "+sq(guest+'\\'+name+'\\*.json*')+" $dst\nCopy-Item "+sq(guest+'\\'+name+'\\runtime*')+' $dst')
    run(['python3',str(scripts/'summarize.py'),str(stage/'results'),'--output',str(a.output)])
finally:
    if guest_created:
        ps("$root="+sq(guest)+"\n$owned=@(Get-CimInstance Win32_Process | Where-Object {$_.ExecutablePath -and $_.ExecutablePath.StartsWith($root+'\\',[StringComparison]::OrdinalIgnoreCase)})\nforeach($p in $owned){Stop-Process -Id $p.ProcessId -Force}\nRemove-Item $root -Recurse -Force")
    if resumed:run(['prlctl','suspend',a.vm])
    shutil.rmtree(stage)
    (a.output/'cleanup.json').write_text(json.dumps({'guest_task_directory_removed':guest_created,'host_staging_removed':True,'vm_restored_to':state,'no_clipboard_read_or_write':True},indent=2)+'\n')
    with (a.output/'SHA256SUMS').open('w') as f:
        for path in sorted(a.output.iterdir()):
            if path.is_file() and path.name!='SHA256SUMS':f.write(hashlib.sha256(path.read_bytes()).hexdigest()+'  '+path.name+'\n')
