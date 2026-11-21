#!/usr/bin/env python3
"""Execute role-aware native loop on the explicitly handed-over Parallels guest."""
import argparse,base64,hashlib,json,shutil,subprocess,uuid
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--stage',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--vm',required=True);p.add_argument('--datasets',nargs='+',default=['images'],choices=['empty','text','images']);p.add_argument('--repetitions',type=int,default=1);p.add_argument('--selections',type=int,choices=range(4),default=0);p.add_argument('--no-captures',action='store_true');p.add_argument('--diagnostic-gc',action='store_true');p.add_argument('--diagnostic-release',action='store_true');p.add_argument('--gui-selections',action='store_true');a=p.parse_args()
stage=a.stage.resolve();out=a.output.resolve()
if out.exists():raise SystemExit('fresh output required')
identity=json.loads((stage/'identity.json').read_text())
if (a.diagnostic_gc or a.diagnostic_release) and not identity.get('attribution'):raise SystemExit('attribution-only commands require --attribution build')
for name,expected in identity['binary_sha256'].items():
 if hashlib.sha256((stage/'bin'/name).read_bytes()).hexdigest()!=expected:raise SystemExit('binary identity mismatch')
out.mkdir(parents=True);(out/'requested-run.json').write_text(json.dumps({'noCaptures':a.no_captures,'diagnosticGC':a.diagnostic_gc,'diagnosticRelease':a.diagnostic_release,'guiSelections':a.gui_selections,'selections':a.selections},indent=2)+'\n');share='\\\\Mac\\Home\\'+str(stage.relative_to(Path.home())).replace('/','\\');guest='C:\\qm'+uuid.uuid4().hex[:6]
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
   try:ps('& '+quote(guest+'\\measure.ps1')+' -Stage '+quote(guest)+' -RunRoot '+quote(result)+' -Dataset '+dataset+(' -SelectionCycles '+str(a.selections) if a.selections else '')+(' -NoCaptures' if a.no_captures else '')+(' -DiagnosticGC' if a.diagnostic_gc else '')+(' -DiagnosticRelease' if a.diagnostic_release else '')+(' -GUISelections' if a.gui_selections else ''))
   finally:
    ps('Copy-Item '+quote(result)+' '+quote(share+'\\'+name)+' -Recurse')
    shutil.copytree(stage/name,out/name);shutil.rmtree(stage/name)
   receipt=json.loads((out/name/'receipt.json').read_text(encoding='utf-8-sig'))
   for key,expected in [('noCaptures',a.no_captures),('diagnosticGC',a.diagnostic_gc),('diagnosticRelease',a.diagnostic_release),('guiSelections',a.gui_selections)]:
    if bool(receipt.get(key,False))!=expected:raise SystemExit('requested/observed flag mismatch: '+key)
   if a.no_captures and list((out/name).glob('*.png')):raise SystemExit('no-capture run produced rasters')
finally:
 ps('$root='+quote(guest)+";$left=@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath -and $_.ExecutablePath.StartsWith($root+'\\',[StringComparison]::OrdinalIgnoreCase)});if($left.Count){throw 'owned processes remain'};Remove-Item $root -Recurse -Force")
 (out/'cleanup.json').write_text(json.dumps({'guest_removed':guest,'stage_retained_for_comparison':str(stage),'vm_state':'running; caller restores original suspended state after cohort'},indent=2))
