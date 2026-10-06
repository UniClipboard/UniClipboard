#!/usr/bin/env python3
"""Static audit of runtime-loaded (dlopen) shared libraries in an extracted AppDir (slice 17c7).

For every ELF in the AppDir: the sonames named in strings (candidate dlopen targets) minus its DT_NEEDED entries, minus the
sonames the AppDir ships. What remains is loaded at run time from the host or not at all. Output is JSON; the verdict per
soname is a human decision recorded in docs/architecture/gui-go-linux-appimage-runtime-deps.md, not computed here. It cannot see modules found by
directory scan (GIO modules, pixbuf loaders, GStreamer plugins): a static aid, never the acceptance (that is the runtime /proc/<pid>/maps in linux_appimage_tls_run.py).
  audit_dlopen.py <AppDir> <out.json>
"""
import json, re, subprocess, sys
from pathlib import Path

root = Path(sys.argv[1]); out = Path(sys.argv[2])
pat = re.compile(rb'(?<![A-Za-z0-9_./-])(lib[A-Za-z0-9_+.-]*\.so(?:\.[0-9]+)*)(?![A-Za-z0-9_])')
shipped = {p.name for p in root.rglob('*.so*') if p.is_file() or p.is_symlink()}
elfs = [p for p in root.rglob('*') if p.is_file() and not p.is_symlink() and p.read_bytes()[:4] == b'\x7fELF']
res = {}
for p in elfs:
    data = p.read_bytes()
    named = {m.group(1).decode() for m in pat.finditer(data)}
    needed = set(re.findall(r'\(NEEDED\)\s+Shared library: \[(.+?)\]', subprocess.run(['readelf', '-d', str(p)], capture_output=True, text=True).stdout))
    left = sorted(named - needed - shipped)
    if left:
        res[str(p.relative_to(root))] = left
agg = {}
for f, libs in res.items():
    for l in libs:
        agg.setdefault(l, []).append(f)
out.write_text(json.dumps({'elfCount': len(elfs), 'shippedCount': len(shipped), 'unresolvedByLibrary': {k: sorted(v) for k, v in sorted(agg.items())}}, indent=1))
print(len(elfs), 'ELF files;', len(agg), 'sonames named but neither NEEDED nor shipped')
