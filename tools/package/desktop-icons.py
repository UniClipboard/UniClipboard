#!/usr/bin/env python3
"""Generate Desktop launcher artwork from the supplied Unic SVG on macOS."""

from pathlib import Path
import subprocess
import tempfile
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[2]
ASSETS = ROOT / "app/desktop/internal/appicons"
SVG = "{http://www.w3.org/2000/svg}"
ET.register_namespace("", SVG[1:-1])
source = ET.parse(ASSETS / "unic-logo.svg").getroot()
mark = source.find(f"{SVG}path")
background = source.find(f"{SVG}rect").attrib["fill"]
master_size = float(source.attrib["width"])


def artwork(canvas, inset, side, radius, edge=""):
    # Keep the original mark-to-background ratio and aspect ratio.
    scale = side / master_size
    return (
        f'<svg xmlns="{SVG[1:-1]}" width="{canvas}" height="{canvas}" '
        f'viewBox="0 0 {canvas} {canvas}">\n'
        f"{edge}"
        f'  <rect x="{inset}" y="{inset}" width="{side}" height="{side}" '
        f'rx="{radius}" fill="{background}"/>\n'
        f'  <path transform="translate({inset} {inset}) scale({scale:.10f})" '
        f'fill="{mark.attrib["fill"]}" fill-rule="{mark.attrib["fill-rule"]}" '
        f'd="{mark.attrib["d"].strip()}"/>\n'
        "</svg>\n"
    )


def render(svg, size, output):
    # Supersample edges at each target size and strip tool-specific metadata.
    document = ET.parse(svg).getroot()
    document.set("width", str(size * 4))
    document.set("height", str(size * 4))
    if size == 16:
        # Snap the ears, whiskers, eyes and mouth to whole pixels.
        path = document.find(f"{SVG}path")
        grid = int(document.attrib["viewBox"].split()[2])
        path.set("transform", f"scale({grid / size:g})")
        path.set(
            "d",
            "M4 4H5V5H6V6H10V5H11V4H12V8H13V9H12V10H13V11H12V12"
            "H4V11H3V10H4V9H3V8H4Z M5 8H6V10H5Z M10 8H11V10H10Z"
            " M7 10H9V11H7Z",
        )
    subprocess.run(
        [
            "magick",
            "-background",
            "none",
            "MSVG:-",
            "-filter",
            "Box",
            "-resize",
            f"{size}x{size}",
            "-strip",
            f"PNG32:{output}",
        ],
        input=ET.tostring(document),
        check=True,
    )


def main():
    with tempfile.TemporaryDirectory() as directory:
        work = Path(directory)
        mac = work / "macos.svg"
        mac.write_text(artwork(1024, 100, 824, 185))
        iconset = work / "Unic.iconset"
        iconset.mkdir()
        for size in (16, 32, 128, 256, 512):
            for density, suffix in ((1, ""), (2, "@2x")):
                render(mac, size * density, iconset / f"icon_{size}x{size}{suffix}.png")
        subprocess.run(
            ["iconutil", "-c", "icns", "-o", ASSETS / "icon.icns", iconset], check=True
        )
        (ASSETS / "icon-macos.png").write_bytes(
            (iconset / "icon_512x512.png").read_bytes()
        )

        windows = work / "windows.svg"
        windows.write_text(
            artwork(
                48,
                3,
                42,
                1,
                '  <rect x="2" y="2" width="44" height="44" rx="2" fill="#808080"/>\n',
            )
        )
        frames = []
        for size in (16, 20, 24, 30, 32, 36, 40, 48, 60, 64, 72, 80, 96, 256):
            frame = work / f"windows-{size}.png"
            render(windows, size, frame)
            frames.append(frame)
        subprocess.run(["magick", *frames, ASSETS / "icon.ico"], check=True)
        (ASSETS / "icon-windows.png").write_bytes(frames[-1].read_bytes())

        linux = ASSETS / "icon-linux.svg"
        linux.write_text(
            artwork(
                128,
                16,
                96,
                12,
                '  <rect x="16" y="16" width="96" height="100" rx="12" fill="#444444"/>\n',
            )
        )
        render(linux, 512, ASSETS / "icon.png")
        render(linux, 48, ASSETS / "icon-linux-48.png")

    subprocess.run(["sh", ROOT / "tools/package/windows-icons.sh"], check=True)


if __name__ == "__main__":
    main()
