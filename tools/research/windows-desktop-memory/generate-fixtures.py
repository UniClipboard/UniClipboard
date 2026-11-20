#!/usr/bin/env python3
"""Generate only seeded synthetic PNGs. Requires the maintained Pillow package."""
import argparse, hashlib, json, random
from pathlib import Path
from PIL import Image
p=argparse.ArgumentParser();p.add_argument('output',type=Path);a=p.parse_args()
a.output.mkdir(parents=True,exist_ok=True)
r=random.Random(20261120);rows=[]
for i in range(12):
    path=a.output/f'image-{i:02}.png'
    im=Image.frombytes('RGB',(2048,1536),r.randbytes(2048*1536*3))
    im.save(path,compress_level=1)
    rows.append(dict(file=path.name,width=2048,height=1536,sha256=hashlib.sha256(path.read_bytes()).hexdigest(),size=path.stat().st_size))
(a.output/'manifest.json').write_text(json.dumps(rows,indent=2)+'\n')
