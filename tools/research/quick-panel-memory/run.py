#!/usr/bin/env python3
"""Execute role-aware native loop on the explicitly handed-over Parallels guest."""
import argparse,base64,hashlib,json,shutil,subprocess,uuid
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--stage',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--vm',required=True);p.add_argument('--datasets',nargs='+',default=['images'],choices=['empty','text','images']);p.add_argument('--repetitions',type=int,default=1);p.add_argument('--selections',type=int,choices=range(4),default=0);a=p.parse_args()
stage=a.stage.resolve();out=a.output.resolve()
if out.exists():raise SystemExit('fresh output required')
identity=json.loads((stage/'identity.json').read_text())
for name,expected in identity['binary_sha256'].items():
 if hashlib.sha256((stage/'bin'/name).read_bytes()).hexdigest()!=expected:raise SystemExit('binary identity mismatch')
out.mkdir(parents=True);share='\\\\Mac\\Home\\'+str(stage.relative_to(Path.home())).replace('/','\\');guest='C:\\qm'+uuid.uuid4().hex[:6]
def quote(x):return "'"+str(x).replace("'","''")+"'"
def ps(t):
 t="$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';[Console]::OutputEncoding=[Text.UTF8Encoding]::new();\n"+t
 subprocess.run(['prlctl','exec',a.vm,'--current-user','powershell.exe','-NoProfile','-ExecutionPolicy','Bypass','-EncodedCommand',base64.b64encode(t.encode('utf-16le')).decode()],check=True)
print('guest='+guest,flush=True)
shutil.copy2(stage/'identity.json',out/'build-identity.json')
shutil.copy2(stage/'fixtures/manifest.json',out/'fixture-manifest.json')
try:
 ps('if(Test-Path '+quote(guest)+"){throw 'collision'};Copy-Item "+quote(share)+' '+quote(guest)+' -Recurse')
 for rep in range(1,a.repetitions+1):
  for dataset in a.datasets:
   name=f'{dataset}-{rep}';print(name,flush=True);result=guest+'\\'+name
   try:ps('& '+quote(guest+'\\measure.ps1')+' -Stage '+quote(guest)+' -RunRoot '+quote(result)+' -Dataset '+dataset+(' -SelectionCycles '+str(a.selections) if a.selections else ''))
   finally:
    ps('Copy-Item '+quote(result)+' '+quote(share+'\\'+name)+' -Recurse')
    shutil.copytree(stage/name,out/name);shutil.rmtree(stage/name)
finally:
 ps('$root='+quote(guest)+";$left=@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath -and $_.ExecutablePath.StartsWith($root+'\\',[StringComparison]::OrdinalIgnoreCase)});if($left.Count){throw 'owned processes remain'};Remove-Item $root -Recurse -Force")
 (out/'cleanup.json').write_text(json.dumps({'guest_removed':guest,'stage_retained_for_comparison':str(stage),'vm_state':'running; caller restores original suspended state after cohort'},indent=2))
