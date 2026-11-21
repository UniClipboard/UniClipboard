#!/usr/bin/env python3
"""Compare real reference/native captures without alignment, resizing or text masks.

Geometry and flat-interior color are hard gates. Full unmasked raster metrics
are retained separately because native and Chromium font rasterizers differ.
"""
import argparse
import collections
import html
import json
from pathlib import Path
from PIL import Image, ImageChops, ImageEnhance, ImageStat


def rect(b):
    return {"x": b.get("x", b.get("X", 0)), "y": b.get("y", b.get("Y", 0)),
            "width": b.get("width", b.get("W", 0)), "height": b.get("height", b.get("H", 0))}


def native_geometry(path):
    rows = json.loads(path.read_text())
    result = {}
    for row in rows:
        name = row.get("Name", "")
        b = rect(row.get("Bounds", {}))
        kind={4:"button",11:"input",15:"dialog",57:"listbox",58:"option",18:"select"}.get(row.get("Role"))
        if row.get("Level",0)>0:kind="heading"
        key=(name,kind)
        if kind and name and b["width"] > 0 and b["height"] > 0:
            if key not in result or b["width"]*b["height"] > result[key]["width"]*result[key]["height"]:
                result[key] = b
    return result


def dominant(image, b, scale):
    # Sample the empty leading interior. A whole-rectangle mode would select
    # white glyph pixels on a continuous gradient and mislabel them as its fill.
    x,y,w,h=b["x"],b["y"],b["width"],b["height"]
    return image.getpixel((round((x+4)*scale),round((y+h/2)*scale)))



def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reference", required=True, type=Path)
    parser.add_argument("--native", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=False)
    rows = []
    scenes=[(f"{theme}-{scale}x-{state}",scale) for theme in ("light","dark") for scale in (1,2) for state in ("dialog","select","invalid")]
    scenes.append(("narrow-1x-dialog",1))
    for stem,scale in scenes:

        rp, np = args.reference/f"{stem}.png", args.native/f"{stem}.png"
        if not np.exists():
            matches=[p for p in args.native.rglob(f"{stem}.png") if "cases" in p.relative_to(args.native).parts]
            if len(matches)==1:np=matches[0]
        if not rp.exists() or not np.exists():
            rows.append({"scene": stem, "status": "FAIL", "error": "required original capture missing"})
            continue
        r, n = Image.open(rp).convert("RGB"), Image.open(np).convert("RGB")
        if r.size != n.size:
            rows.append({"scene": stem, "status": "FAIL", "error": f"image sizes differ: {r.size} versus {n.size}"})
            continue
        diff = ImageChops.difference(r, n)
        overlay = Image.blend(r,n,.5)
        pair = Image.new("RGB", (r.width*2,r.height));pair.paste(r);pair.paste(n,(r.width,0))
        pair.save(args.out/f"{stem}-side-by-side.png")
        overlay.save(args.out/f"{stem}-overlay.png")
        diff.save(args.out/f"{stem}-absolute-difference.png")
        ImageEnhance.Contrast(diff).enhance(6).save(args.out/f"{stem}-heatmap.png")
        geometry_path=args.reference/f"{stem}-geometry.json"
        access_path=np.with_name(f"{stem}-access.json")
        if not geometry_path.exists() or not access_path.exists():
            rows.append({"scene":stem,"status":"FAIL","error":"required geometry missing"});continue
        ref=json.loads(geometry_path.read_text()); refrows=ref.get("elements",[]) if isinstance(ref,dict) else ref
        actual=native_geometry(access_path)
        measurements=[]
        for element in refrows:
            name=element.get("name"); kind=element.get("kind"); expected=rect(element.get("bounds",{}))
            # React Aria's 1px screen-reader dismiss buttons are not visual
            # controls. The launcher is inert outside the modal scope.
            if expected["width"]<=1 or expected["height"]<=1 or name=="Open settings":continue
            key=(name,kind)
            if key not in actual:
                measurements.append({"name":name,"status":"FAIL","error":"native semantic control missing"});continue
            got=actual[key];delta={k:round(got[k]-expected[k],4) for k in expected}
            geometry_pass=max(abs(v) for v in delta.values())<=.5
            rc,nc=dominant(r,expected,scale),dominant(n,got,scale)
            color_error=max(abs(a-b) for a,b in zip(rc,nc)) if rc and nc else None
            # Text-only header bounds use font rasterization evidence, not a background gate.
            color_pass=kind=="heading" or (color_error is not None and color_error<=2)
            measurements.append({"name":name,"status":"PASS" if geometry_pass and color_pass else "FAIL", "reference":expected,"native":got,"delta_dip":delta,"dominant_reference_rgb":rc,"dominant_native_rgb":nc,"dominant_color_max_error":color_error,"color_gate_scope":"text-only; see unmasked raster metrics" if kind=="heading" else "leading empty control interior","reference_font":element.get("font"),"reference_colors":element.get("colors")})
        rows.append({"scene":stem,"status":"PASS" if measurements and all(x["status"]=="PASS" for x in measurements) else "FAIL", "unmasked_mean_absolute_rgb_error":ImageStat.Stat(diff).mean,"unmasked_rms_rgb_error":ImageStat.Stat(diff).rms,"geometry_tolerance_dip":.5,"flat_color_tolerance":2,"measurements":measurements})
    result={"status":"PASS" if all(x["status"]=="PASS" for x in rows) and len(rows)==13 else "FAIL", "scope":"same-host Chromium versus mygo software renderer; no OS compositor acceptance; raster metrics are unmasked and descriptive", "scenes":rows}
    (args.out/"comparison.json").write_text(json.dumps(result,indent=2)+"\n")
    body=["<!doctype html><meta charset=utf-8><title>BoardUI comparison</title><style>body{font:14px system-ui;margin:24px}img{max-width:100%}pre{white-space:pre-wrap}</style>",f"<h1>BoardUI: {result['status']}</h1>","<p>Left: actual official React. Right: native mygo. Originals, overlay and absolute differences remain separate files.</p>"]
    for row in rows:
        stem=row['scene'];body += [f"<h2>{html.escape(stem)}: {row['status']}</h2>",f'<img src="{stem}-side-by-side.png">',f'<p><a href="{stem}-overlay.png">Overlay</a> · <a href="{stem}-heatmap.png">Difference heatmap</a></p>',"<pre>"+html.escape(json.dumps(row,indent=2))+"</pre>"]
    (args.out/"index.html").write_text("\n".join(body))
    print(args.out/"comparison.json")
    return 0 if result['status']=="PASS" else 1

if __name__=="__main__":
    raise SystemExit(main())
