#!/usr/bin/env python3
"""Capture the official fixture through the documented ego-browser API."""
import argparse,json,pathlib,re,subprocess
p=argparse.ArgumentParser();p.add_argument('--out',required=True);p.add_argument('--port',type=int,default=4177);p.add_argument('--resources',action='store_true');p.add_argument('--only-resources',action='store_true');p.add_argument('--space',type=int,default=0);a=p.parse_args()
here=pathlib.Path(__file__).resolve().parent;root=here.parents[3]
out=pathlib.Path(a.out).resolve()
if out.exists() and any(out.iterdir()):raise SystemExit('Refuse to overwrite an existing capture directory: '+str(out))
out.mkdir(parents=True,exist_ok=True)
constants={'BOARDUI_REPO':str(root),'BOARDUI_OUTPUT':str(pathlib.Path(a.out).resolve()),'BOARDUI_PORT':a.port,'BOARDUI_RESOURCES':a.resources or a.only_resources,'BOARDUI_ONLY_RESOURCES':a.only_resources,'BOARDUI_SPACE':a.space}
script='\n'.join('const '+k+'='+json.dumps(v)+';' for k,v in constants.items())+'\n'+(here/'capture.mjs').read_text()
result=subprocess.run(['ego-browser','nodejs'],input=script,text=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
(out/'reference-capture.log').write_text(result.stdout)
print(result.stdout,end='')
matches=re.findall(r'"taskSpaceId"\s*:\s*(\d+)',result.stdout)
space_id=int(matches[0]) if matches else None
receipt={'task_space_id':space_id,'capture_exit_code':result.returncode,'cleanup_exit_code':None,'cleanup':'NOT_RUN'}
if result.returncode!=0:
    receipt['reason']='Capture failed; preserve the owned TaskSpace for diagnosis.'
    (out/'reference-cleanup.json').write_text(json.dumps(receipt,indent=2)+'\n')
    raise SystemExit(result.returncode)
if space_id is None or len(set(matches))!=1:
    receipt['reason']='Capture did not identify exactly one owned TaskSpace; refuse broad cleanup.'
    (out/'reference-cleanup.json').write_text(json.dumps(receipt,indent=2)+'\n')
    raise SystemExit(receipt['reason'])
cleanup_script='const task=await taskSpace('+str(space_id)+');\nawait task.finish({keep:[]});\nconsole.log(JSON.stringify({phase:"reference-cleanup-complete",taskSpaceId:'+str(space_id)+'}));\n'
cleanup=subprocess.run(['ego-browser','nodejs'],input=cleanup_script,text=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
(out/'reference-cleanup.log').write_text(cleanup.stdout)
print(cleanup.stdout,end='')
receipt['cleanup_exit_code']=cleanup.returncode
receipt['cleanup']='PASS' if cleanup.returncode==0 else 'FAIL'
(out/'reference-cleanup.json').write_text(json.dumps(receipt,indent=2)+'\n')
raise SystemExit(cleanup.returncode)
