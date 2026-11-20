#!/usr/bin/env python3
"""Run the existing image acceptance on Windows with prebuilt task-owned programs."""
import argparse,hashlib,json,os,shutil,subprocess,tempfile
from pathlib import Path
from replay import ROOT,IMPLEMENTATION,run,ps,quote,digest

p=argparse.ArgumentParser(description=__doc__);p.add_argument('--vm',default='Windows 11');p.add_argument('--stage',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args();stage=a.stage.resolve();out=a.output.resolve()
if stage.exists() or out.exists():raise SystemExit('Stage and output must be fresh')
if not stage.is_relative_to(Path.home()):raise SystemExit('Stage must be under shared home')
stage.mkdir(parents=True);out.mkdir(parents=True)
identity={'source_sha':subprocess.check_output(['git','rev-parse',IMPLEMENTATION],cwd=ROOT,text=True).strip(),'go':subprocess.check_output(['go','version'],text=True).strip(),'race_detector':False,'race_reason':'Go1.27 Windows ARM64 race detector is unsupported','native_ui':False,'scope':'existing canonical real-service image scenario on Windows, software views, private profiles; no physical input or clipboard','test_adapter':['use task-prebuilt unicd instead of invoking host go build -race','use task TEMP instead of /tmp','explicit private unic migrate --initialize-empty before service start','disable automatic sync in synthetic profile'],'source_sha256':{}}
created=False;resumed=False;guest='C:\\uimc-db66';share='\\\\Mac\\Home\\'+str(stage.relative_to(Path.home())).replace('/','\\')
try:
 with tempfile.TemporaryDirectory(prefix='unic-image-win-acceptance-') as scratch:
  src=Path(scratch);archive=subprocess.Popen(['git','archive',IMPLEMENTATION],cwd=ROOT,stdout=subprocess.PIPE);run(['tar','-x','-C',str(src)],stdin=archive.stdout);archive.stdout.close()
  if archive.wait():raise RuntimeError('Source export failed')
  f=src/'app/desktop/internal/desktopui/acceptance_images_test.go';text=f.read_text();start=text.index('\texe := filepath.Join(private, "unicd")');end=text.index('\tbinary, err := os.ReadFile(exe)',start)
  text=text[:start]+'''    exe:=os.Getenv("UNIC_MEMORY_SERVICE_BINARY")
    if exe==""{t.Fatal("task service binary missing")}
'''+text[end:]
  text=text.replace('os.MkdirTemp("/tmp", "unic-wake-")','os.MkdirTemp(os.TempDir(), "unic-wake-")').replace('[sync]\\nenabled=false','[sync]\\nautomatic=false')
  needle='\tif err := proc.Start(); err != nil {'
  primer='''    initialize:=exec.CommandContext(ctx,filepath.Join(filepath.Dir(exe),"unic.exe"),"migrate","--initialize-empty","--json")
    initialize.Env=proc.Env
    if output,err:=initialize.CombinedOutput();err!=nil{t.Fatalf("explicit initialization: %v: %s",err,output)}
'''
  if text.count(needle)!=2:raise RuntimeError('Service fixture seam changed')
  f.write_text(text.replace(needle,primer+needle,1))
  identity['source_sha256']={str(p.relative_to(src)):digest(p) for p in (src/'app/desktop/internal/desktopui').glob('live*.go')}
  identity['adapted_acceptance_sha256']=digest(f)
  env=dict(os.environ,GOOS='windows',GOARCH='arm64',CGO_ENABLED='0')
  for name in ['unic','unicd']:run(['go','build','-trimpath','-ldflags=-s -w','-o',str(stage/(name+'.exe')),'./app/service/cmd/'+name],cwd=src,env=env)
  run(['go','test','-c','-trimpath','-o',str(stage/'image-acceptance.exe'),'./app/desktop/internal/desktopui'],cwd=src,env=env)
 identity['program_sha256']={p.name:digest(p) for p in stage.glob('*.exe')};(out/'identity.json').write_text(json.dumps(identity,indent=2)+'\n')
 info=subprocess.check_output(['prlctl','list','-i',a.vm],text=True)
 if 'State: suspended' in info:run(['prlctl','resume',a.vm]);resumed=True
 elif 'State: running' not in info:raise RuntimeError('Unexpected VM state')
 ps(a.vm,"if(Get-CimInstance Win32_Process | Where-Object {$_.Name -match '^unic(-desktop|d)?\\.exe$'}){throw 'Another task owns the application'}\nif(Test-Path "+quote(guest)+"){throw 'Guest task path exists'}")
 created=True;ps(a.vm,'Copy-Item '+quote(share)+' '+quote(guest)+' -Recurse')
 source="$root="+quote(guest)+r'''
Add-Type -TypeDefinition 'using System.Runtime.InteropServices; public static class ClipboardSequenceOnly { [DllImport("user32.dll")] public static extern uint GetClipboardSequenceNumber(); }'
$sequenceBefore=[ClipboardSequenceOnly]::GetClipboardSequenceNumber()
foreach($dir in @('temp','appdata','local','user')){New-Item ($root+'\'+$dir) -ItemType Directory | Out-Null}
$env:TEMP=$root+'\temp';$env:TMP=$env:TEMP;$env:APPDATA=$root+'\appdata';$env:LOCALAPPDATA=$root+'\local';$env:USERPROFILE=$root+'\user'
$env:UNIC_DESKTOP_E2E='1';$env:UNIC_DESKTOP_E2E_CASE_RECEIPT=$root+'\receipt.json';$env:UNIC_MEMORY_SERVICE_BINARY=$root+'\unicd.exe'
& ($root+'\image-acceptance.exe') '-test.run=^TestDesktopAcceptance$/^image-previews$' '-test.v' '-test.timeout=4m' > ($root+'\driver.txt')
$code=$LASTEXITCODE
[pscustomobject]@{before=$sequenceBefore;after=[ClipboardSequenceOnly]::GetClipboardSequenceNumber();contentReadOrWritten=$false} | ConvertTo-Json | Set-Content ($root+'\clipboard-sequence.json') -Encoding UTF8
Copy-Item ($root+'\clipboard-sequence.json') DEST_CLIP
Copy-Item ($root+'\receipt.json') DEST_RECEIPT
Copy-Item ($root+'\driver.txt') DEST_LOG
if($code -ne 0){throw ('Windows image acceptance failed: '+$code)}
'''
 source=source.replace('DEST_RECEIPT',quote(share+'\\receipt.json')).replace('DEST_LOG',quote(share+'\\driver.txt')).replace('DEST_CLIP',quote(share+'\\clipboard-sequence.json'))
 (stage/'run.ps1').write_text(source,encoding='utf-8')
 print(ps(a.vm,'& '+quote(share+'\\run.ps1')))
 receipt=json.loads((stage/'receipt.json').read_text(encoding='utf-8-sig'));(out/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
 (out/'clipboard-sequence.json').write_text((stage/'clipboard-sequence.json').read_text(encoding='utf-8-sig'))
 (out/'driver-output-sha256.json').write_text(json.dumps({'sha256':digest(stage/'driver.txt'),'bytes':(stage/'driver.txt').stat().st_size})+'\n')
 if not receipt.get('passed'):raise RuntimeError('Receipt failed')
except BaseException as exc:
 (out/'failure.json').write_text(json.dumps({'type':type(exc).__name__,'message':str(exc),'output':getattr(exc,'output','')},default=str)+'\n');raise
finally:
 if created:
  ps(a.vm,"$root="+quote(guest)+"\n$owned=@(Get-CimInstance Win32_Process | Where-Object {$_.ExecutablePath -and $_.ExecutablePath.StartsWith($root+'\\',[StringComparison]::OrdinalIgnoreCase)})\nforeach($p in $owned){$process=Get-Process -Id $p.ProcessId;Stop-Process -Id $p.ProcessId -Force;$process.WaitForExit(10000)|Out-Null}\nif(Test-Path $root){Remove-Item $root -Recurse -Force}")
 if resumed:run(['prlctl','suspend',a.vm])
 shutil.rmtree(stage)
 (out/'cleanup.json').write_text(json.dumps({'guest_task_removed':created,'host_stage_removed':True,'vm_resuspended':resumed,'system_clipboard_used':False})+'\n')
 with (out/'SHA256SUMS').open('w') as f:
  for p in sorted(out.iterdir()):
   if p.is_file() and p.name!='SHA256SUMS':f.write(digest(p)+'  '+p.name+'\n')
