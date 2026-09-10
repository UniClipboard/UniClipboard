"""Validate unshadowed history and item-preview screenshots at any display scale."""
import sys
from PIL import Image

history = Image.open(sys.argv[1]).convert("RGBA")
scale = history.width / 360
assert abs(history.height / scale - 420) < 2, "Unexpected history window size"
for point in [(history.width//2,2),(history.width//2,history.height-3),(2,history.height//2),(history.width-3,history.height//2)]:
    assert history.getpixel(point)[3] >= 230, "History has unexpected outer padding"
if len(sys.argv) > 2:
    preview = Image.open(sys.argv[2]).convert("RGBA")
    assert abs(preview.width / scale - 368) < 2, "Unexpected preview width"
    assert 94 <= preview.height / scale <= 482, "Unexpected adaptive preview height"
    gutter_alpha = min(preview.getpixel((x,y))[3] for x in [2,preview.width-3] for y in [preview.height//4,preview.height*3//4])
    assert gutter_alpha < 100, "Pointer gutter has a rectangular background"
    assert preview.getpixel((preview.width//2,preview.height//2))[3] >= 230, "Preview body is transparent"
print("Panel surfaces and adaptive preview bounds are valid")
