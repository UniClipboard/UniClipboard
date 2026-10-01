"""Generate synthetic images for the memory probe. Usage: make_memory_images.py <output-dir>"""
import os
import sys
from pathlib import Path
from PIL import Image, ImageDraw

out = Path(sys.argv[1])
out.mkdir(parents=True, exist_ok=True)


def noisy(name, size):
    # Random pixels keep the PNG as large as a real Retina screenshot (about 20 MB at 3456 x 2234).
    image = Image.frombytes("RGB", size, os.urandom(size[0] * size[1] * 3))
    ImageDraw.Draw(image).rectangle((20, 20, size[0] - 21, size[1] - 21), outline="white", width=6)
    image.save(out / f"{name}.png")


def graded(name, size, shift):
    image = Image.new("RGB", size)
    draw = ImageDraw.Draw(image)
    for y in range(size[1]):
        t = y / max(1, size[1] - 1)
        draw.line((0, y, size[0], y), fill=(int(40 + 150 * t), int(90 + shift), int(160 - 90 * t)))
    for x in range(0, size[0], 97):
        draw.line((x, 0, x, size[1]), fill=(255, 255, 255), width=1)
    image.save(out / f"{name}.png")


for n in range(8):
    noisy(f"large-{n}", (3456, 2234))
for n in range(50):
    graded(f"medium-{n}", (1280, 720), n * 3)
frames = [Image.new("RGB", (800, 600), (i * 20 % 255, 80, 160)) for i in range(12)]
frames[0].save(out / "animated.gif", save_all=True, append_images=frames[1:], duration=80, loop=0)
