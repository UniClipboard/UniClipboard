"""Generate deterministic visual fixtures for sizing, alpha, and zoom checks."""
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

output = Path(__file__).parent / "images"
output.mkdir(exist_ok=True)
font_path = Path(__file__).parent.parent / "assets/fonts/Inter.ttf"

def grid(name, size, start, end):
    image = Image.new("RGB", size)
    draw = ImageDraw.Draw(image)
    for y in range(size[1]):
        t = y / max(1, size[1] - 1)
        color = tuple(round(a + (b - a) * t) for a, b in zip(start, end))
        draw.line((0, y, size[0], y), fill=color)
    font = ImageFont.truetype(str(font_path), max(20, size[0] // 30))
    draw.rectangle((8, 8, size[0] - 9, size[1] - 9), outline="white", width=6)
    for x in range(0, size[0], 200):
        draw.line((x, 0, x, size[1]), fill=(130, 190, 200), width=2)
    for y in range(0, size[1], 200):
        draw.line((0, y, size[0], y), fill=(130, 190, 200), width=2)
    draw.text((40, 35), name.upper(), font=font, fill="white")
    draw.text((40, size[1] - font.size - 45), f"{size[0]} x {size[1]}", font=font, fill="white")
    draw.ellipse((size[0] * .3, size[1] * .3, size[0] * .7, size[1] * .7), fill=(246, 202, 122))
    image.save(output / f"{name}.png")

grid("landscape", (2560, 1440), (20, 97, 120), (13, 34, 62))
grid("portrait", (1000, 2400), (78, 84, 155), (37, 27, 65))
grid("small", (120, 80), (30, 110, 140), (30, 70, 90))
alpha = Image.new("RGBA", (800, 800))
draw = ImageDraw.Draw(alpha)
draw.rounded_rectangle((100, 100, 700, 700), radius=100, fill=(34, 170, 152, 180))
draw.ellipse((225, 225, 575, 575), fill=(250, 199, 94, 255))
alpha.save(output / "transparent.png")
