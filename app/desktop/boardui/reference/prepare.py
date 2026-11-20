#!/usr/bin/env python3
"""Prepare the immutable official source in an ignored reference-only sandbox."""
import hashlib,json,pathlib,shutil,subprocess,sys
ROOT=pathlib.Path(__file__).resolve().parents[4]
HERE=pathlib.Path(__file__).resolve().parent
BUILD=ROOT/'.build/boardui-reference'
SHA='ec5c5eb3e29c2441be63b9422cd54f411ef23466'
def run(args,**kw):subprocess.run(args,check=True,**kw)
BUILD.mkdir(parents=True,exist_ok=True)
source=BUILD/'upstream'
if not (source/'.git').exists():run(['git','clone','--no-checkout','https://github.com/BoardUI/boardui.git',str(source)])
run(['git','-C',str(source),'checkout','--detach',SHA])
actual=subprocess.check_output(['git','-C',str(source),'rev-parse','HEAD'],text=True).strip()
if actual!=SHA:raise SystemExit('Official source identity mismatch')
research=json.loads((ROOT/'docs/exec/boardui-mygo/sources.json').read_text())
for entry in research['boardui_git']['files']:
 p=source/entry['path']
 if hashlib.sha256(p.read_bytes()).hexdigest()!=entry['sha256']:raise SystemExit('Modified source: '+str(p))
node=shutil.which('node')
if not node:raise SystemExit('Node 22.12+ required')
version=subprocess.check_output([node,'--version'],text=True).strip()
parts=tuple(map(int,version.lstrip('v').split('.')))
if parts<(22,12,0):raise SystemExit('Use Node 22.12+; found '+version)
(BUILD/'node-path.txt').write_text(node+'\n')
for name in ['package.json','package-lock.json','vite.config.mjs','index.html','fixture.css','fixture.tsx']:
 shutil.copy2(HERE/name,BUILD/name)
shutil.copy2(HERE.parent/'assets/Inter.ttf',BUILD/'Inter.ttf')
npm=pathlib.Path(node).with_name('npm')
if not (BUILD/'node_modules/vite').is_dir():
 run([str(npm),'ci','--ignore-scripts','--no-fund','--no-audit','--cache',str(BUILD/'npm-cache')],cwd=BUILD,env={**__import__('os').environ,'PATH':str(pathlib.Path(node).parent)+':'+__import__('os').environ['PATH']})
print(json.dumps({'root':str(BUILD),'source':SHA,'node':node,'node_version':version,'port':4177}))
