#!/usr/bin/env python3
"""Generate BoardUI-styled, text-free macOS DMG background artwork on macOS."""

from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
ASSETS = ROOT / "packaging/macos"

WIDTH = 600
HEIGHT = 380

SVG = f"""<svg xmlns="http://www.w3.org/2000/svg" width="{WIDTH}" height="{HEIGHT}" viewBox="0 0 {WIDTH} {HEIGHT}">
  <defs>
    <radialGradient id="ambient" cx="50%" cy="18%" r="68%">
      <stop offset="0%" stop-color="#2b7fff" stop-opacity="0.06"/>
      <stop offset="100%" stop-color="#2b7fff" stop-opacity="0"/>
    </radialGradient>
    <linearGradient id="accentPill" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="#2b7fff"/>
      <stop offset="100%" stop-color="#155dfc"/>
    </linearGradient>
    <filter id="cardShadow" x="-8%" y="-8%" width="116%" height="124%">
      <feDropShadow dx="0" dy="8" stdDeviation="12" flood-color="#0a0a0a" flood-opacity="0.06"/>
      <feDropShadow dx="0" dy="1" stdDeviation="1.5" flood-color="#0a0a0a" flood-opacity="0.04"/>
    </filter>
    <filter id="pillShadow" x="-30%" y="-30%" width="160%" height="180%">
      <feDropShadow dx="0" dy="4" stdDeviation="5" flood-color="#155dfc" flood-opacity="0.24"/>
    </filter>
  </defs>

  <!-- BoardUI light secondary canvas (#f7f7f7) -->
  <rect width="{WIDTH}" height="{HEIGHT}" fill="#f7f7f7"/>
  <rect width="{WIDTH}" height="{HEIGHT}" fill="url(#ambient)"/>

  <!-- Elevated BoardUI dialog card (dialog-radius: 24) -->
  <rect x="28" y="28" width="544" height="324" rx="24" fill="#ffffff" stroke="#ebebeb" stroke-width="1" filter="url(#cardShadow)"/>

  <!-- Left source slot (frames UniClipboard.app + label) -->
  <rect x="78" y="92" width="172" height="196" rx="20" fill="#fafafa" stroke="#ebebeb" stroke-width="1"/>

  <!-- Right drop-target slot (frames Applications + label) -->
  <rect x="350" y="92" width="172" height="196" rx="20" fill="#f4f8ff" stroke="#8ec5ff" stroke-width="1.5" stroke-dasharray="6 5"/>

  <!-- Center directional guidance (BoardUI primary accent, zero text) -->
  <line x1="258" y1="176" x2="342" y2="176" stroke="#bedbff" stroke-width="1.5" stroke-dasharray="4 4" stroke-linecap="round"/>
  <circle cx="300" cy="176" r="26" fill="#eff6ff"/>
  <circle cx="300" cy="176" r="20" fill="url(#accentPill)" filter="url(#pillShadow)"/>
  <circle cx="300" cy="176" r="19.5" fill="none" stroke="#ffffff" stroke-opacity="0.26" stroke-width="1"/>
  <path d="M293 176 H307 M301.5 170.5 L307 176 L301.5 181.5" fill="none" stroke="#ffffff" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/>
</svg>
"""

SWIFT_RENDERER = r"""
import AppKit
import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

func color(_ r: CGFloat, _ g: CGFloat, _ b: CGFloat, _ a: CGFloat = 1.0) -> CGColor {
    CGColor(colorSpace: CGColorSpace(name: CGColorSpace.sRGB)!, components: [r / 255.0, g / 255.0, b / 255.0, a])!
}

func render(scale: Int, output: String) {
    let width = 600
    let height = 380
    let pw = width * scale
    let ph = height * scale
    let cs = CGColorSpace(name: CGColorSpace.sRGB)!
    guard let ctx = CGContext(
        data: nil,
        width: pw,
        height: ph,
        bitsPerComponent: 8,
        bytesPerRow: 0,
        space: cs,
        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
    ) else {
        fatalError("failed to create CGContext")
    }
    ctx.setAllowsAntialiasing(true)
    ctx.setShouldAntialias(true)
    ctx.interpolationQuality = .high

    // Use top-left coordinate system matching SVG / Finder window coordinates.
    ctx.translateBy(x: 0, y: CGFloat(ph))
    ctx.scaleBy(x: CGFloat(scale), y: -CGFloat(scale))

    // 1. Base BoardUI secondary canvas (#F7F7F7)
    ctx.setFillColor(color(247, 247, 247))
    ctx.fill(CGRect(x: 0, y: 0, width: width, height: height))

    // 2. Subtle top-center ambient accent glow
    let ambientColors = [color(43, 127, 255, 0.06), color(43, 127, 255, 0.0)] as CFArray
    if let ambient = CGGradient(colorsSpace: cs, colors: ambientColors, locations: [0.0, 1.0]) {
        ctx.drawRadialGradient(
            ambient,
            startCenter: CGPoint(x: 300, y: 68),
            startRadius: 0,
            endCenter: CGPoint(x: 300, y: 68),
            endRadius: 360,
            options: [.drawsAfterEndLocation]
        )
    }

    // 3. Elevated BoardUI dialog card (rx=24) with subtle layered shadow
    let cardRect = CGRect(x: 28, y: 28, width: 544, height: 324)
    let cardPath = CGPath(roundedRect: cardRect, cornerWidth: 24, cornerHeight: 24, transform: nil)

    ctx.saveGState()
    ctx.setShadow(offset: CGSize(width: 0, height: -8), blur: 24, color: color(10, 10, 10, 0.06))
    ctx.setFillColor(color(255, 255, 255))
    ctx.addPath(cardPath)
    ctx.fillPath()
    ctx.restoreGState()

    ctx.saveGState()
    ctx.setShadow(offset: CGSize(width: 0, height: -1), blur: 3, color: color(10, 10, 10, 0.04))
    ctx.setFillColor(color(255, 255, 255))
    ctx.addPath(cardPath)
    ctx.fillPath()
    ctx.restoreGState()

    ctx.setStrokeColor(color(235, 235, 235))
    ctx.setLineWidth(1.0)
    ctx.addPath(cardPath)
    ctx.strokePath()

    // 4. Left source bay (UniClipboard.app + label)
    let leftRect = CGRect(x: 78, y: 92, width: 172, height: 196)
    let leftPath = CGPath(roundedRect: leftRect, cornerWidth: 20, cornerHeight: 20, transform: nil)
    ctx.setFillColor(color(250, 250, 250))
    ctx.addPath(leftPath)
    ctx.fillPath()
    ctx.setStrokeColor(color(235, 235, 235))
    ctx.setLineWidth(1.0)
    ctx.addPath(leftPath)
    ctx.strokePath()

    // 5. Right drop-target bay (Applications + label)
    let rightRect = CGRect(x: 350, y: 92, width: 172, height: 196)
    let rightPath = CGPath(roundedRect: rightRect, cornerWidth: 20, cornerHeight: 20, transform: nil)
    ctx.setFillColor(color(244, 248, 255))
    ctx.addPath(rightPath)
    ctx.fillPath()
    ctx.saveGState()
    ctx.setStrokeColor(color(142, 197, 255))
    ctx.setLineWidth(1.5)
    ctx.setLineDash(phase: 0, lengths: [6, 5])
    ctx.addPath(rightPath)
    ctx.strokePath()
    ctx.restoreGState()

    // 6. Center dashed guide line
    ctx.saveGState()
    ctx.setStrokeColor(color(190, 219, 255))
    ctx.setLineWidth(1.5)
    ctx.setLineCap(.round)
    ctx.setLineDash(phase: 0, lengths: [4, 4])
    ctx.move(to: CGPoint(x: 258, y: 176))
    ctx.addLine(to: CGPoint(x: 342, y: 176))
    ctx.strokePath()
    ctx.restoreGState()

    // 7. Outer accent-50 halo ring
    let haloRect = CGRect(x: 300 - 26, y: 176 - 26, width: 52, height: 52)
    ctx.setFillColor(color(239, 246, 255))
    ctx.fillEllipse(in: haloRect)

    // 8. BoardUI primary-gradient directional action circle
    let pillRect = CGRect(x: 300 - 20, y: 176 - 20, width: 40, height: 40)
    let pillPath = CGPath(ellipseIn: pillRect, transform: nil)

    ctx.saveGState()
    ctx.setShadow(offset: CGSize(width: 0, height: -4), blur: 10, color: color(21, 93, 252, 0.24))
    ctx.setFillColor(color(21, 93, 252))
    ctx.addPath(pillPath)
    ctx.fillPath()
    ctx.restoreGState()

    ctx.saveGState()
    ctx.addPath(pillPath)
    ctx.clip()
    let pillColors = [color(43, 127, 255), color(21, 93, 252)] as CFArray
    if let pillGrad = CGGradient(colorsSpace: cs, colors: pillColors, locations: [0.0, 1.0]) {
        ctx.drawLinearGradient(
            pillGrad,
            start: CGPoint(x: 300, y: 156),
            end: CGPoint(x: 300, y: 196),
            options: []
        )
    }
    ctx.restoreGState()

    // Inner subtle highlight ring
    let innerRing = CGRect(x: 300 - 19.5, y: 176 - 19.5, width: 39, height: 39)
    ctx.setStrokeColor(color(255, 255, 255, 0.26))
    ctx.setLineWidth(1.0)
    ctx.strokeEllipse(in: innerRing)

    // Crisp white right arrow (BoardUI 2px rounded-cap stroke language)
    ctx.saveGState()
    ctx.setStrokeColor(color(255, 255, 255))
    ctx.setLineWidth(2.2)
    ctx.setLineCap(.round)
    ctx.setLineJoin(.round)
    ctx.move(to: CGPoint(x: 293, y: 176))
    ctx.addLine(to: CGPoint(x: 307, y: 176))
    ctx.move(to: CGPoint(x: 301.5, y: 170.5))
    ctx.addLine(to: CGPoint(x: 307, y: 176))
    ctx.addLine(to: CGPoint(x: 301.5, y: 181.5))
    ctx.strokePath()
    ctx.restoreGState()

    guard let image = ctx.makeImage() else {
        fatalError("failed to extract CGImage")
    }
    let url = URL(fileURLWithPath: output)
    guard let dest = CGImageDestinationCreateWithURL(url as CFURL, UTType.png.identifier as CFString, 1, nil) else {
        fatalError("failed to create PNG destination")
    }
    let dpi = 72 * scale
    let props: [CFString: Any] = [
        kCGImagePropertyDPIWidth: dpi,
        kCGImagePropertyDPIHeight: dpi,
    ]
    CGImageDestinationAddImage(dest, image, props as CFDictionary)
    if !CGImageDestinationFinalize(dest) {
        fatalError("failed to write PNG")
    }
}

let args = CommandLine.arguments
guard args.count == 3 else {
    fatalError("usage: render.swift <1x.png> <2x.png>")
}
render(scale: 1, output: args[1])
render(scale: 2, output: args[2])
"""


def main():
    ASSETS.mkdir(parents=True, exist_ok=True)
    (ASSETS / "dmg-background.svg").write_text(SVG, encoding="utf-8")
    with tempfile.TemporaryDirectory(prefix="unic-dmg-art-") as directory:
        work = Path(directory)
        script = work / "render.swift"
        script.write_text(SWIFT_RENDERER, encoding="utf-8")
        png1 = work / "bg.png"
        png2 = work / "bg@2x.png"
        subprocess.run(["swift", str(script), str(png1), str(png2)], check=True)
        raw_tiff = work / "raw.tiff"
        subprocess.run(
            ["tiffutil", "-cathidpicheck", str(png1), str(png2), "-out", str(raw_tiff)],
            check=True,
        )
        # Compress multi-page TIFF with LZW if sips/tiffutil supports it; tiffutil -lzw preserves multi-res pages.
        out_tiff = ASSETS / "dmg-background.tiff"
        subprocess.run(
            ["tiffutil", "-lzw", str(raw_tiff), "-out", str(out_tiff)],
            check=True,
        )
    print(ASSETS / "dmg-background.tiff")


if __name__ == "__main__":
    main()
