"""Real daemon resource acceptance, called only through diagnostics_acceptance_run.py."""
import os
import sys
import tempfile
import subprocess
import time
import json
import zipfile
import hashlib
import urllib.request
import signal
import ctypes
from pathlib import Path
from run import ROOT, isolated_env
from file_preview_run import cli, PASSPHRASE

WORKER_CODE = 'import sys,time,gc\nmemory=None\nprint("ready",flush=True)\nfor command in sys.stdin:\n if command.strip()=="load":\n  memory=bytearray(128*1024*1024)\n  end=time.monotonic()+9\n  while time.monotonic()<end: pass\n  print("loaded",flush=True)\n elif command.strip()=="release":\n  memory=None;gc.collect();print("released",flush=True)\n elif command.strip()=="quit":break\n'

def run(out):
    out = Path(out).resolve()
    out.mkdir(parents=True, exist_ok=False)
    home=tempfile.mkdtemp(prefix='uc-gui-go-resource-load-');profile='gui-go-'+Path(home).name
    env=isolated_env(home,profile,{'PATH':str(ROOT/'target/debug')+':'+os.environ['PATH']})
    report={'desktopHead':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),'enginePin':next(line for line in (ROOT/'Cargo.toml').read_text().splitlines() if line.startswith('uc-engine =')),'passed':False,'fixture':'task-owned synthetic registered host; not a product GUI claim','phases':{},'samePidReuse':'NOT_RUN','actualSleep':'NOT_RUN'}
    worker=None;fixtures=[];stopped=False
    def spawn():
     p=subprocess.Popen([sys.executable,'-u','-c',WORKER_CODE],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
     assert p.stdout.readline().strip()=='ready';return p

    def register(pid):
     body=json.dumps({'pid':pid,'clientType':'gui'}).encode()
     req=urllib.request.Request(base+'/auth/connect',data=body,headers={'Authorization':'Bearer '+conn['token'],'Content-Type':'application/json'})
     with urllib.request.urlopen(req,timeout=3) as response:assert response.status==200

    def export(name):
     result=json.loads(cli(env,'--json','debug','export-logs').stdout)
     dest=out/(name+'.zip');dest.write_bytes(Path(result['path']).read_bytes());return dest

    def records(path):
     with zipfile.ZipFile(path) as z:
      return [json.loads(l) for l in z.read('otel/metrics.jsonl').splitlines()],json.loads(z.read('otel/manifest.json'))

    def for_pid(rows,pid):
     found=[]
     for r in rows:
      for rm in r['resourceMetrics']:
       attrs={a['key']:next(iter(a['value'].values())) for a in rm['resource']['attributes']}
       if int(attrs['process.pid'])==pid:
        for sm in rm['scopeMetrics']:
         for metric in sm['metrics']:
          for kind in ['gauge','sum']:
           for point in metric.get(kind,{}).get('dataPoints',[]):
            found.append({'name':metric['name'],'at':int(point['timeUnixNano']),'value':point.get('asDouble',point.get('asInt')),'state':next(a['value']['stringValue'] for a in point['attributes'] if a['key']=='uc.sample.state'),'resourceRunId':attrs['service.instance.id']})
     return found

    try:
     cli(env,'space','init','--passphrase',PASSPHRASE,'--device-name','synthetic-resource-fixture');cli(env,'start')
     conn=json.loads((Path(home)/'Library/Application Support'/('app.uniclipboard.desktop-'+profile)/'daemon.conn').read_text());base='http://'+conn['host']+':'+str(conn['port'])
     cli(env,'debug','capture','start','--minutes','2')
     worker=spawn();pid=worker.pid;register(pid)
     report['phases']['baselineAtNs']=time.time_ns();time.sleep(3)
     report['phases']['loadAtNs']=time.time_ns();worker.stdin.write('load\n');worker.stdin.flush();assert worker.stdout.readline().strip()=='loaded'
     report['phases']['releaseAtNs']=time.time_ns();worker.stdin.write('release\n');worker.stdin.flush();assert worker.stdout.readline().strip()=='released';time.sleep(3)
     path=export('load');rows,meta=records(path);points=for_pid(rows,pid)
     cpu=[float(p['value']) for p in points if p['name']=='uc.process.cpu.utilization'];mem=[float(p['value']) for p in points if p['name']=='process.memory.usage']
     assert max(cpu)>.5,(cpu,meta)
     assert max(mem)-min(mem)>64*1024*1024,mem
     first=min(p['at'] for p in points);assert all(p['name']!='uc.process.cpu.utilization' for p in points if p['at']==first)
     report.update(peakSingleCoreFraction=max(cpu),residentIncreaseBytes=max(mem)-min(mem),hostFirstCpuOmitted=True)
     worker.stdin.write('quit\n');worker.stdin.flush();worker.wait(timeout=5);report['phases']['exitedAtNs']=time.time_ns();time.sleep(2)
     rows,meta=records(export('exited'));assert any(h['pid']==pid and h['state']=='exited' for h in meta['history']['hostCoverage'])
     report['exitedHostMarked']=True
     worker=spawn();newpid=worker.pid;register(newpid);time.sleep(2)
     rows,meta=records(export('restarted'));newpoints=for_pid(rows,newpid);first=min(p['at'] for p in newpoints);assert all(p['name']!='uc.process.cpu.utilization' for p in newpoints if p['at']==first)
     assert newpoints[0]['resourceRunId']!=points[0]['resourceRunId'];report['newProcessBaselineReset']=True
     # The daemon was started only for this profile. Verify kernel executable identity before each signal.
     lib=ctypes.CDLL('/usr/lib/libproc.dylib');lib.proc_pidpath.argtypes=[ctypes.c_int,ctypes.c_void_p,ctypes.c_uint32]
     expected=(ROOT/'target/debug/uniclipd').resolve()
     def verified_signal(sig):
      buf=ctypes.create_string_buffer(4096);assert lib.proc_pidpath(conn['pid'],buf,len(buf))>0
      assert Path(os.fsdecode(buf.value)).resolve()==expected
      os.kill(conn['pid'],sig)
     verified_signal(signal.SIGSTOP);stopped=True;time.sleep(17);verified_signal(signal.SIGCONT);stopped=False;time.sleep(2)
     rows,meta=records(export('gap'));daemonpoints=for_pid(rows,conn['pid']);gap=[p for p in daemonpoints if p['state']=='gap'];assert gap and all(p['name']!='uc.process.cpu.utilization' for p in gap);report['schedulerGapCpuOmitted']=True
     # Quota applies to explicitly registered owned processes, never a global process scan.
     for i in range(33):
      p=subprocess.Popen(['/bin/sleep','60']);fixtures.append(p);register(p.pid)
     rows,meta=records(export('quota'));assert len(meta['history']['hostCoverage'])<=32 and meta['history']['hostRegistrationFailures']>0
     report['registrationQuotaBounded']=True;report['hostRegistrationFailures']=meta['history']['hostRegistrationFailures']
     report['passed']=True
     (out/'points.json').write_text(json.dumps(points,indent=2)+'\n')
    finally:
     if stopped:verified_signal(signal.SIGCONT)
     if worker is not None and worker.poll() is None:
      worker.stdin.write('quit\n');worker.stdin.flush();worker.wait(timeout=5)
     for fixture in fixtures:
      if fixture.poll() is None:fixture.terminate();fixture.wait(timeout=5)
     stop=cli(env,'stop',check=False);report['daemonStopExit']=stop.returncode
     report['fixtureCleanup']='all exited and reaped'
     (out/'result.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report))
