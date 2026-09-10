"""Convert the existing web fonts to CoreText-compatible assets."""
from pathlib import Path
from shutil import copyfile
from fontTools.ttLib import TTFont

target = Path("apps/gpui-quick-panel/assets/fonts")
target.mkdir(parents=True, exist_ok=True)
for package, source, name in [
    ("@fontsource-variable/inter", "inter-latin-wght-normal.woff2", "Inter"),
    ("@fontsource/jetbrains-mono", "jetbrains-mono-latin-400-normal.woff2", "JetBrainsMono"),
]:
    package_dir = Path("node_modules") / package
    font = TTFont(package_dir / "files" / source)
    font.flavor = None
    font.save(target / f"{name}.ttf")
    copyfile(package_dir / "LICENSE", target / f"{name}-LICENSE.txt")
