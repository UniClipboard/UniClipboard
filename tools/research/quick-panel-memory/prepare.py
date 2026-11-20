#!/usr/bin/env python3
"""Freeze current source and build opt-in isolated role measurements. No VM access."""
import argparse, hashlib, io, json, os, shutil, struct, subprocess, tarfile
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--stage',type=Path,required=True);p.add_argument('--source');p.add_argument('--projection-fix',action='store_true');p.add_argument('--arch',choices=['arm64','amd64'],default='arm64');a=p.parse_args()
repo=Path(__file__).resolve().parents[3];stage=a.stage.resolve()
if stage.exists():raise SystemExit('stage must be fresh')
stage.mkdir(parents=True);src=stage/'source';src.mkdir()
if a.source:
 commit=subprocess.check_output(['git','rev-parse',a.source+'^{commit}'],cwd=repo,text=True).strip()
 with tarfile.open(fileobj=io.BytesIO(subprocess.check_output(['git','archive',commit],cwd=repo))) as archive:archive.extractall(src,filter='data')
 hashes={str(f.relative_to(src)):hashlib.sha256(f.read_bytes()).hexdigest() for f in src.rglob('*') if f.is_file()}
else:
 files=subprocess.check_output(['git','ls-files','-c','-o','--exclude-standard','-z'],cwd=repo).decode().split('\0')
 hashes={}
 for rel in filter(None,files):
  f=repo/rel
  if not f.is_file():continue
  if not f.resolve().is_relative_to(repo):raise SystemExit('source escapes repository')
  dst=src/rel;dst.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(f,dst);hashes[rel]=hashlib.sha256(f.read_bytes()).hexdigest()
 commit=subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip()
if a.projection_fix:
 f=src/'app/desktop/internal/desktopui/quick_panel_model.go';t=f.read_text();needle='\t\tp.entries = nil\n'
 if t.count(needle)!=1:raise SystemExit('projection control seam changed')
 t=t.replace(needle,needle+'\t\t// Reopening the same query can reuse an unchanged service revision.\n\t\t// Rebuild the cleared projection even when no new result serial arrives.\n\t\tp.live.seen = 0\n');f.write_text(t);hashes[str(f.relative_to(src))]=hashlib.sha256(f.read_bytes()).hexdigest()
research=repo/'tools/research';diag=(research/'windows-desktop-memory/diagnostic_init.go.txt').read_text()
diag=diag.replace('\"pid\":os.Getpid(),','\"utcMillis\":time.Now().UnixMilli(),\"pid\":os.Getpid(),')
diag=diag.replace('path+"."+filepath.Base(os.Args[0])+".jsonl"','path+"."+filepath.Base(os.Args[0])+"-"+strconv.Itoa(os.Getpid())+".jsonl"').replace('"runtime"','"runtime"\n "strconv"')
for rel in ['app/desktop/cmd/unic-desktop','app/service/cmd/unicd']:(src/rel/'memory_research.go').write_text(diag)
probe=(research/'desktop-image-memory/cache_probe.go.txt').read_text().replace('path+".cache.jsonl"','path+"."+strconv.Itoa(os.Getpid())+".cache.jsonl"').replace('"reflect"','"reflect";"strconv"')
probe=probe.replace('\"elapsedSeconds\":time.Since(start).Seconds(),','\"utcMillis\":time.Now().UnixMilli(),\"elapsedSeconds\":time.Since(start).Seconds(),')
probe=probe.replace('\"reflect\"','\"reflect\";\"weak\"').replace('var memoryResearchActive *Desktop','var memoryResearchActive weak.Pointer[viewState]').replace('memoryResearchActive=s','memoryResearchActive=weak.Make(s.d)').replace('s:=memoryResearchActive;if s==nil||s.d.live==nil{return};d,l:=s.d,s.d.live','d:=memoryResearchActive.Value();if d==nil||d.live==nil{return};l:=d.live')
(src/'app/desktop/internal/desktopui/memory_research.go').write_text(probe)
for rel,old,new in [('live.go','func (s *Desktop) View(c *ui.Context) { s.d.view(c) }','func (s *Desktop) View(c *ui.Context) { memoryResearchSeen(s); s.d.view(c) }'),('live_quick.go','d := q.d\n','d := q.d\n memoryResearchSeen(&Desktop{d:d})\n')]:
 f=src/'app/desktop/internal/desktopui'/rel;t=f.read_text()
 if t.count(old)!=1:raise SystemExit('diagnostic seam changed: '+rel)
 f.write_text(t.replace(old,new))
panel=src/'app/desktop/internal/desktopui/process_panel.go'
t=panel.read_text();needle='func (p *ProcessPanel) Open(target quickpanel.Target, captureError string) {'
if t.count(needle)!=1:raise SystemExit('panel diagnostic seam changed')
panel.write_text(t.replace(needle,needle+'\n memorySelectionID=\"\";memorySelectionReported=false'))
bindir=stage/'bin';bindir.mkdir();env=dict(os.environ,GOOS='windows',GOARCH=a.arch,CGO_ENABLED='0')
for name,path,tags in [('unic','./app/service/cmd/unic',[]),('unicd','./app/service/cmd/unicd',[]),('unic-desktop','./app/desktop/cmd/unic-desktop',['-tags','desktop_e2e']),('helper','./app/desktop/e2e/testdata/windows-native/helper.go',[])]:
 subprocess.run(['go','build','-trimpath','-buildvcs=false',*tags,'-ldflags=-s -w'+(' -H windowsgui' if name=='unic-desktop' else ''),'-o',str(bindir/(name+'.exe')),path],cwd=src,env=env,check=True)
for name in ['native.cs','window-state.cs','capture-window.ps1','generate-fixtures.py']:shutil.copy2(research/'windows-desktop-memory'/name,stage/name)
shutil.copy2(Path(__file__).with_name('measure.ps1'),stage/'measure.ps1')
subprocess.run(['python3',str(stage/'generate-fixtures.py'),str(stage/'fixtures')],check=True)
identity={'source_head':commit,'control_projection_fix':a.projection_fix,'dirty':bool(a.projection_fix) if a.source else subprocess.check_output(['git','status','--porcelain'],cwd=repo,text=True),'source_sha256':hashes,'arch':a.arch,'race':False,'build_flags':['-trimpath','-buildvcs=false','-ldflags=-s -w','desktop: -tags desktop_e2e -H windowsgui'],'cgo_enabled':False,'instrumented_source_sha256':{str(f.relative_to(src)):hashlib.sha256(f.read_bytes()).hexdigest() for f in [src/'app/desktop/internal/desktopui'/n for n in ['live.go','live_quick.go','process_panel.go']]},'pe_machine':{f.name:hex(struct.unpack_from('<H',f.read_bytes(),struct.unpack_from('<I',f.read_bytes(),0x3c)[0]+4)[0]) for f in bindir.iterdir()},'binary_sha256':{f.name:hashlib.sha256(f.read_bytes()).hexdigest() for f in bindir.iterdir()},'go':subprocess.check_output(['go','version'],text=True).strip(),'diagnostic_sha256':{str(f.relative_to(src)):hashlib.sha256(f.read_bytes()).hexdigest() for f in src.rglob('memory_research.go')}}
(stage/'identity.json').write_text(json.dumps(identity,indent=2)+'\n');shutil.rmtree(src)
print(stage)
