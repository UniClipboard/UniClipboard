#!/usr/bin/env python3
"""Real native WebView E2E for the bounded history window.

Requires an acceptance app, built daemon and Go CLI. Uses only synthetic images,
a throwaway HOME and a unique profile; system clipboard access stays disabled.
"""
import argparse
import base64
import hashlib
import json
import os
import re
import socket
import shutil
import subprocess
import sys
import tempfile
import time
import tomllib
import traceback
import urllib.request
import urllib.error
from pathlib import Path

p = argparse.ArgumentParser()
p.add_argument('--grid', action='store_true')
p.add_argument('--large-viewport', action='store_true')
p.add_argument('--repo', required=True, type=Path)
binary_args=p.add_mutually_exclusive_group(required=True)
binary_args.add_argument('--app', type=Path)
binary_args.add_argument('--gui', type=Path)
p.add_argument('--out', required=True, type=Path)
a = p.parse_args()
repo = a.repo.resolve()
sys.path.insert(0, str(repo / 'apps/gui-go/e2e'))
from history_kit_browser_daemon import start, stop, env_for
from file_preview_run import PASSPHRASE, cli
from run import isolated_env
from thumbnail_resource_run import png

a.out.parent.mkdir(parents=True, exist_ok=True)
result = {'desktop_sha': subprocess.check_output(['git','rev-parse','HEAD'], cwd=repo, text=True).strip(),
          'driver_sha256': hashlib.sha256((repo / 'apps/gui-go/frontend/src/e2e/history-window.ts').read_bytes()).hexdigest(),
          'runner_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(), 'gui_started': False, 'passed': False, 'platform':sys.platform, 'large_viewport':a.large_viewport,
          'image_dimensions': [512,384], 'image_count': 120, 'view': 'grid' if a.grid else 'list', 'steps': []}
result['engine_revision'] = tomllib.loads((repo/'Cargo.toml').read_text())['workspace']['dependencies']['uc-engine']['rev']
result['desktop_worktree_clean'] = not subprocess.check_output(['git','status','--porcelain'], cwd=repo, text=True).strip()
result['source_patch_sha256'] = hashlib.sha256(subprocess.check_output(['git','diff','HEAD'],cwd=repo)).hexdigest()
proc = None
portable_env = None
with tempfile.TemporaryDirectory(prefix='uc-gui-go-memory-') as temporary:
    tmp=Path(temporary)
    evidence=tmp/'native.jsonl'
    evidence.write_text('')
    control_file=tmp/'control.txt'
    control_file.write_text('')
    try:
        if a.gui:
            # Linux isolation requires a complete portable installation, not only a temporary HOME.
            for source,name in [(a.gui.resolve(),'gui-go'),(repo/'target/gui-go/uniclip','uniclip'),(repo/'target/debug/uniclipd','uniclipd')]:
                shutil.copy2(source,tmp/name)
            profile='gui-go-'+tmp.name
            portable_env=isolated_env(str(tmp),profile,{'UC_PORTABLE':'1','PATH':str(tmp)+os.pathsep+os.environ['PATH']})
            def cli(env,*args,timeout=120,check=True):
                response=subprocess.run([str(tmp/'uniclip'),*args],env=env,capture_output=True,text=True,timeout=timeout)
                if check and response.returncode: raise RuntimeError(f'isolated CLI failed: {response.stderr}')
                return response
            cli(portable_env,'space','init','--passphrase',PASSPHRASE,'--device-name','history-window')
            cli(portable_env,'start')
            connection=next((tmp/'data').rglob('daemon.conn'))
            value=json.loads(connection.read_text())
            info={'home':str(tmp),'profile':profile,'conn':str(connection),'daemonUrl':f"http://{value.get('host','127.0.0.1')}:{value['port']}"}
        else:
            start(tmp, 0, True)
            info=json.loads((tmp/'daemon.json').read_text())
        env=portable_env or env_for(info['home'],info['profile'])
        with socket.socket() as sock:
            sock.bind(('127.0.0.1',0)); port=sock.getsockname()[1]
        setup=json.loads(cli(env,'--json','mobile','setup','--non-interactive','--label','memory-test','--ip','127.0.0.1','--accept-network-risk','--port',str(port)).stdout)
        auth='Basic '+base64.b64encode(f"{setup['username']}:{setup['password']}".encode()).decode()
        for _ in range(100):
            try:
                with socket.create_connection(('127.0.0.1',port), timeout=1): break
            except OSError: time.sleep(.1)
        hashes=[]
        for i in range(120):
            name=f'memory-{i:02}.png'; image=png(512,384,i); hashes.append(hashlib.sha256(image).hexdigest())
            for path,body,kind in [('/file/'+name,image,'image/png'),('/SyncClipboard.json',json.dumps({'type':'Image','text':name,'dataName':name,'hasData':True}).encode(),'application/json')]:
                req=urllib.request.Request(f'http://127.0.0.1:{port}{path}',data=body,method='PUT',headers={'Authorization':auth,'Content-Type':kind})
                with urllib.request.urlopen(req,timeout=120) as response: assert response.status < 300
        result['image_sha256']=hashes
        conn=json.loads(Path(info['conn']).read_text())
        def daemon_request(path, headers, body=None):
            request=urllib.request.Request(info['daemonUrl']+path, data=body, headers=headers)
            with urllib.request.urlopen(request,timeout=120) as response: return json.load(response)['data']
        session=daemon_request('/auth/connect', {'Authorization':'Bearer '+conn['token'],'Content-Type':'application/json'}, json.dumps({'pid':os.getpid(),'clientType':'gui'}).encode())['sessionToken']
        session_headers={'Authorization':'Session '+session}
        unlocked=daemon_request('/content-lock/unlock', {**session_headers,'Content-Type':'application/json'}, json.dumps({'passphrase':PASSPHRASE}).encode())
        assert unlocked.get('unlocked') is True, 'content grant was not unlocked'
        result['content_unlocked_before_history']=True
        entries=daemon_request('/clipboard/entries?limit=200&offset=0',session_headers)
        entries=[entry for entry in entries if entry.get('contentType')=='image']
        assert len(entries)==120, f'expected 120 images, got {len(entries)}'
        started=time.monotonic(); remaining={entry['id'] for entry in entries}
        while remaining and time.monotonic()-started < 120:
            for entry_id in list(remaining):
                try:
                    resource=daemon_request('/clipboard/entries/'+entry_id+'/resource',session_headers)
                    if resource.get('url'): remaining.remove(entry_id)
                except urllib.error.HTTPError as error:
                    # The original may still be materializing after the capture was listed.
                    if error.code not in (404, 409, 503):
                        raise
            if remaining: time.sleep(1)
        result['source_ready_count']=120-len(remaining)
        result['source_ready_wait_seconds']=round(time.monotonic()-started,2)
        if remaining: raise RuntimeError(f'{len(remaining)} original resources did not become ready')
        search=daemon_request('/search/query?query=&limit=200&offset=0',session_headers)
        ordered_ids=[item['entryId'] for item in search['items']]
        desktop_keys=('DISPLAY','XAUTHORITY','WAYLAND_DISPLAY','XDG_RUNTIME_DIR','XDG_SESSION_TYPE','DBUS_SESSION_BUS_ADDRESS','GDK_BACKEND','WEBKIT_DISABLE_DMABUF_RENDERER')
        gui_env=isolated_env(info['home'], info['profile'], {**({'UC_PORTABLE':'1'} if a.gui else {}),**{key:os.environ[key] for key in desktop_keys if key in os.environ},'PATH': str(repo/'target/debug')+os.pathsep+os.environ['PATH'], 'UC_GUI_GO_ISOLATED':'1', 'UC_GUI_GO_EVIDENCE':str(evidence),'UC_GUI_GO_E2E_PHASE':'history-window','UC_GUI_GO_E2E_SECRET':json.dumps({'ids':ordered_ids,'grid':a.grid,'largeViewport':a.large_viewport}),'UC_GUI_GO_E2E_CONTROL_FILE':str(control_file),'UC_GUI_GO_E2E_VISIBLE':'1','UC_GUI_GO_EXIT_MODE':'full','UC_GPUI_QUICK_PANEL':'0'})
        binary=(tmp/'gui-go') if a.gui else a.app/'Contents/MacOS/gui-go'
        result['gui_sha256']=hashlib.sha256(binary.read_bytes()).hexdigest()
        result['daemon_sha256']=hashlib.sha256(((repo/'target/debug/uniclipd') if a.gui else (a.app/'Contents/MacOS/uniclipd')).read_bytes()).hexdigest()
        with (tmp/'gui.log').open('w') as log:
            proc=subprocess.Popen([str(binary)],env=gui_env,stdout=log,stderr=subprocess.STDOUT)
            result['gui_started']=True
            beginning=time.monotonic(); deadline=beginning+180
            inserted_requests=0
            while time.monotonic()<deadline and proc.poll() is None:
                rows=[json.loads(x) for x in evidence.read_text().splitlines() if x.strip()]
                if any(row.get('step')=='window-begin' for row in rows) and not control_file.read_text():
                    control_file.write_text('open-main history-window\n')
                if any(row.get('step')=='window-large-viewport' for row in rows):
                    result['passed']=any(row.get('step')=='window-large-viewport' and row.get('ok') for row in rows)
                    result['native_steps']=rows
                    break
                requested=sum(row.get('step')=='window-insert-request' for row in rows)
                if requested > inserted_requests:
                    name=f'window-insert-{inserted_requests}.png'
                    image=png(512,384,120+inserted_requests)
                    for path,body,kind in [('/file/'+name,image,'image/png'),('/SyncClipboard.json',json.dumps({'type':'Image','text':name,'dataName':name,'hasData':True}).encode(),'application/json')]:
                        request=urllib.request.Request(f'http://127.0.0.1:{port}{path}',data=body,method='PUT',headers={'Authorization':auth,'Content-Type':kind})
                        with urllib.request.urlopen(request,timeout=120) as response: assert response.status < 300
                    inserted_requests+=1
                result['native_steps']=rows
                result['steps']=[x for x in rows if str(x.get('step','')).startswith(('history-window','image-memory','window-','driver-error'))]
                if any(x.get('step')=='driver-error' for x in result['steps']): break
                if any(x.get('step')=='image-memory-idle' for x in result['steps']):
                    result['passed']=all(any(x.get('step')==name and x.get('ok') for x in result['steps']) for name in ['image-memory-loaded','image-memory-scroll-0','image-memory-scroll-1','image-memory-scroll-2','image-memory-all-ready','image-memory-idle'])
                    break
                time.sleep(1)
            result['elapsed_seconds']=round(time.monotonic()-beginning,2)
    except Exception as exc:
        result['error']=f'{type(exc).__name__}: {exc}'
        result['traceback']=traceback.format_exc()
    finally:
        result['gui_exit_before_cleanup']=proc.poll() if proc else None
        if proc and proc.poll() is None:
            proc.terminate()
            try: proc.wait(timeout=15)
            except subprocess.TimeoutExpired: result['gui_stop']='termination timed out'
        if (tmp/'gui.log').exists():
            text=(tmp/'gui.log').read_text(errors='replace')
            for secret in (locals().get('session'), locals().get('auth'), locals().get('conn',{}).get('token'), locals().get('setup',{}).get('password')):
                if secret: text=text.replace(str(secret),'[redacted]')
            result['gui_log']=re.sub(r'(?i)(Bearer|Session)\s+[A-Za-z0-9_.-]+',r'\1 [redacted]',text)[-16000:]
        try:
            if portable_env:
                stopped=cli(portable_env,'stop',check=False,timeout=60)
                result['daemon_stop_code']=stopped.returncode
            elif (tmp/'daemon.json').exists(): stop(tmp)
        except Exception as exc: result['daemon_stop_error']=str(exc)
        a.out.write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps({'passed': result['passed'],'error': result.get('error'),'output':str(a.out)}))
sys.exit(0 if result['passed'] else 1)
