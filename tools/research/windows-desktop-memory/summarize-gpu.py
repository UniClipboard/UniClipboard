#!/usr/bin/env python3
"""Summarize GPU object-area estimates separately from process memory."""
import argparse,hashlib,json
from pathlib import Path
from PIL import Image

p=argparse.ArgumentParser();p.add_argument('input',type=Path);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
result=[];crops={}
for folder in sorted(a.input.glob('gpu-native-*-v2')):
    if not (folder/'identity.json').exists():continue
    ident=json.loads((folder/'identity.json').read_text(encoding='utf-8-sig'))
    if not ident.get('reopenVerified'):continue
    samples=json.loads((folder/'samples.json').read_text(encoding='utf-8-sig'))
    traces=[json.loads(line) for line in (folder/'runtime.gpu.jsonl').read_text().splitlines()]
    phases=[]
    for name in dict.fromkeys(r['phase'] for r in samples):
        rows=[r for r in samples if r['phase']==name and r['role']=='desktop']
        end=max(r['utcMillis'] for r in rows)
        last={}
        for r in traces:
            if r['unixMillis']<=end:last[r['hwnd']]=r
        keys=['imageTextures','imageRGBAAreaBytesEstimate','maskAtlasBytesEstimate','colorAtlasBytesEstimate','swapchainBytesEstimate']
        phases.append({'phase':name,'totals':{k:sum(r[k] for r in last.values()) for k in keys},'renderers':[dict(r,secondsSincePresentation=(end-r['unixMillis'])/1000) for r in last.values()]})
    pixels={}
    for name in ['selected.png','restored-native.png']:
        path=folder/name
        if path.exists():
            im=Image.open(path).convert('RGB')
            # Fixed logical crop(800,130)-(1040,280) at verified DPI192.
            # This lies wholly inside the selected synthetic image.
            crop=im.crop((1600,260,2080,560));digest=hashlib.sha256(crop.tobytes()).hexdigest()
            pixels[name]={'clientWidth':im.width,'clientHeight':im.height,'nativeImageCropSHA256':digest,'wholeCaptureSHA256':hashlib.sha256(path.read_bytes()).hexdigest()}
            crops[folder.name+'/'+name]=digest
    result.append({'run':folder.name,'desktopPID':ident['desktopPID'],'binarySHA256':ident['desktopSHA256'],'quickVisibleAtCheckpoint':ident.get('quickVisibleAtCheckpoint'),'pixelChecks':pixels,'phases':phases})
verified=len(crops)>=4 and len(set(crops.values()))==1
out={'schema':1,'units':'estimated pixel-area bytes and texture counts, not VRAM/residency','nativeCropPixels':[1600,260,2080,560],'allFourNativeCropsEqual':verified,'cropHashes':crops,'runs':result}
a.output.write_text(json.dumps(out,separators=(',',':'))+'\n')
print('native crop equality:',verified,'runs:',len(result))
