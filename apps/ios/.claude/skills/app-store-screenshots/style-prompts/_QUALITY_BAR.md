# Universal Quality Bar (read before any style spec)

These rules apply to **every** style in this folder. They override slide-specific instructions only when violated by them. If your output fails any of these, it is **not done**.

> Canvas reference: every rule below is calibrated to the App Store iPhone canvas **1320 × 2868** (6.9"). For other canvas sizes, scale proportionally by canvas width.

---

## 1. Composition (no dead space, no clipping)

- **Phone must occupy 68–82% of the canvas vertical height** when a phone is present (spec-permitted "no-device" slides exempt). Below 65% means the phone reads as small and the slide reads as empty.
- **Headline + phone together must occupy at least 80% of canvas height**. The remaining 20% is split between top margin (above headline) and bottom margin (below phone). No single empty band > 22% of canvas height.
- **Headlines must auto-fit horizontally**. If a single word's measured width > 88% of canvas width at the chosen font size, you MUST do one of: (a) break the word onto its own line; (b) reduce font size to fit; (c) hyphenate. Letters running off-canvas is an instant fail. Verify with a measurement pass — do not eyeball.
- **Use up to 3 visual layers max per slide**: background → phone/illustration → headline + 1 accent. Above this becomes noise.
- **Never center-vertically a phone with empty bands above AND below**. Anchor the phone to one edge (top bleed, bottom bleed, or flush bottom).

## 2. Illustration & SVG quality bar (anti-clipart)

The single biggest failure mode is "Figma-quick-draw" mascots and props that read as clipart. Every illustrated element must clear at least **3 of these 5 polish gates**:

1. **Two or more fill tones** per major shape — a body shape gets a base fill PLUS a 5–15% darker shadow side AND/OR a 5–10% lighter highlight rim. Single-flat-fill shapes are clipart.
2. **A cast shadow on the ground** — an elliptical drop shadow under standing characters/objects at 10–25% black opacity, 60–80% of object width, blurred 6–10px. Floats without shadows look pasted in.
3. **A surface texture** — halftone dots, paper grain, film noise, micro-stipple, or specular streak — whichever the style demands. At least 4% opacity, never zero.
4. **Volume cue** — at minimum an inner-shadow on the shaded side, a specular highlight on the lit side, OR a contour line that thickens on the shadow side. Flat outlined shapes with no volume cue look like icons.
5. **Detail density appropriate to scale** — a mascot rendered at 300px on a 1320px canvas needs visible facial expression (eyes with highlights, mouth with depth, cheeks/blush). The viewer should not be able to count the shapes in 1 second.

A 30-second sanity test: if you cropped the mascot/illustration out and put it next to a piece of art from the style's named inspiration (Cancoco, Mate, Superlist, etc.), would it look like it belongs in the same product, or would it look like a knock-off? If knock-off, re-render.

## 3. Mascot & illustration sizing minimums

When a style features a mascot/character/object, it must be **physically large on the canvas** — viewers see thumbnails:

| Element                                   | Min size on 1320×2868 canvas        |
|-------------------------------------------|--------------------------------------|
| Headlining mascot (cover slide)           | **≥ 380px** tall (≥ 13% canvas H)    |
| Companion / supporting mascots            | ≥ 260px tall                         |
| Mascot inside-phone (within UI)           | ≥ 120px tall                         |
| Hero 3D object (camera/globe/book/etc.)   | ≥ 500px in its largest dimension     |
| Floating chip / sticker (decorative)      | 100–260px depending on role          |
| Apple Watch macro (when called for)       | ≥ 520px tall                         |

If the spec gives a specific size, that size wins. Otherwise, use these minimums.

## 4. Typography contrast & legibility

- All headline text must clear **WCAG AA 4.5:1** against the local pixel underneath it. If your headline sits over a gradient or photo, sample the **worst** pixel under the letter shapes — not the average — and verify against that.
- **The "emphasis word" trap.** Accent / italic / colored emphasis words are the most common contrast failure point: they're often a pale tint of the brand color (lilac on cream, mustard on sage, peach on tan) chosen for *style* without measuring contrast. **Every emphasis color must independently clear 4.5:1 against its slide bg.** If it doesn't, swap to a darker variant of the same hue family — never just "hope the user catches it." Examples that historically failed:
  - lilac `#B49BE6` on cotton-candy gradient → 2.3:1 → FAIL, must darken to deep violet ~`#5B3FC8`
  - mustard `#F2BB46` on sage `#BFD9B6` → 1.7:1 → FAIL, must switch hue to coral `#E45A4A` (~4.1:1) or darken to brown-mustard `#7A4F1B`
  - any pale color over a 3-stop pastel gradient → ALWAYS verify against the lightest stop, not the average
- If contrast falls below 4.5:1 anywhere under the headline, you MUST do one of: (a) swap the color to a darker/different hue; (b) add a scrim (linear gradient overlay) under the text; (c) add a text-shadow large enough to lift contrast (2–4px offset, 30–45% alpha — not a "subtle" shadow that disappears in thumbnails); (d) move the headline off the busy area. Never ship low-contrast text and hope.
- **Headline-over-decoration rule**: if a 3D object (globe, mascot, glossy numeral, etc.) sits in the same vertical band as the headline, the headline letters MUST NOT cross the object's silhouette. Either move the object or move the headline — overlap is always a contrast disaster.
- Inside floating chips and stickers, body text must clear **4.5:1**. Tiny labels (< 12px equivalent) must clear **7:1**.
- **Subtitle / body copy on textured bg** (cork mosaic, paper grain, photo, gradient) must be either (a) larger than 36px equivalent at 1320px canvas width AND ≥ 4.5:1, OR (b) sit on a clean rounded scrim card. Small navy text on cork mosaic at 28px = unreadable at thumbnail size.
- Never use pure `#FFFFFF` text on a pure-saturated mid-tone background (e.g., white on `#7BC9F5` cyan) — drop to a near-white tinted with the bg's complement, or add a scrim.

## 5. Headline composition rules

- **Max characters per line, soft cap by canvas width**: `floor(canvasWidth / fontSize × 0.45)` chars. For a 96px Inter Bold headline on 1320px canvas, that's ~13 chars/line including spaces. Going over forces clipping risk.
- **Max 3 lines** for any headline. 2 is better. 1 is allowed only if the line fits comfortably in < 80% canvas width.
- **Hand-break lines for visual rhythm**. Each line should be approximately the same width OR alternate short-long deliberately. Avoid one fat line and one tiny widow line.
- **Trailing punctuation**: follow the style's spec. Default to a single period at end of full sentences; never a question mark in marketing headlines unless the spec allows it; never multiple exclamation marks.
- **Tracking and leading for big display type**: leading should be ≤ 1.05 (lines should kiss), tracking should be slightly negative (`-0.01em` to `-0.03em`). Loose default browser leading at display size always looks amateur.

## 6. Phone treatment

- **Bezel sizing matches the iPhone 15 Pro silhouette**: corner radius scales with phone width. At 920px phone width (≈70% canvas), use a 52–56px outer radius and a 36–42px inner screen radius.
- **Phone screenshot fill**: the user-supplied screenshot must fill the inner screen edge-to-edge with no white margin and no visible status-bar duplication. If the screenshot has a transparent background, that's a bug — flatten it first.
- **Tilt range**: respect the spec. If the spec calls for upright (0°), stay upright. If the spec allows tilt, stay within the stated range; never exceed it.
- **Phone drop shadow tinted with the bg**: pure black shadow on a colored bg looks pasted-in. Tint the shadow with the bg complement at 10–20% opacity. Two-shadow stack always (a long soft shadow + a tight contact shadow).
- **Use the default iPhone bezel.** Every phone-bearing slide MUST render the user's screenshot inside the template's default `Phone` device frame (the iPhone mockup PNG at `public/mockup.png` driven by the `Phone` component in `src/components/editor/device-frames.tsx`). **Do NOT** replace it with a bezelless rounded rectangle, a paper cutout, a cream paper border, or any custom-drawn frame. The bezel is the unifying visual across the whole skill output — strip it and the screenshots lose the iPhone read at thumbnail size. Per-style spec sheets may tint or shadow the area around the bezel, but they must NOT replace the bezel itself.

## 7. Background quality

- **No accidental gradient on flat-color slides** — if the spec says "flat" the bg must be a single hex, no gradient at all. Inverse: if the spec calls for a gradient, the gradient must use at least 3 stops with intentional hue shift, not a 2-stop near-identical-color gradient.
- **Noise / grain texture** at the opacity the spec requires. 0% noise on styles that demand grain (Retro Rubberhose, Hand-Drawn Editorial, Moody Curated Dating) is a fail.
- **Vignette only when the spec calls for it**. Adding a vignette to a Glassy Iridescent slide kills the brightness; omitting it on Moody Curated Dating kills the atmosphere. Follow the spec.

## 8. Decoration density discipline

Per slide:
- **Minimal styles** (Crisp Teal Bezelless Wallet, Editorial Minimal, Moody Curated Dating): **0** floating decorations.
- **Editorial styles** (Hand-Drawn Editorial, Soft Sunset): **2–5** decorations, intentionally placed.
- **Maximalist styles** (Glassy Iridescent Social, Paper Sticker Skeuomorphic, Glossy 3D K-Beauty): **8–14** decorations, scattered with at least one bleeding off-canvas.

Decoration density wrong-side either way (sparse maximalist or dense minimalist) reads as a style misfire.

## 9. Auto-reject checks (run before declaring done)

Before exporting, run through this list. Any "yes" means re-render:

- [ ] Any text running off-canvas or clipped by a slide edge?
- [ ] Any phone smaller than 65% of canvas height (on a phone-bearing slide)?
- [ ] Any empty band wider than 22% of canvas height?
- [ ] Any mascot/object below the minimum size for its role?
- [ ] Any headline text failing WCAG AA contrast at its hardest point against the bg?
- [ ] Any illustrated element failing 3+ of the 5 polish gates?
- [ ] Any pure-white text on a saturated mid-tone bg without scrim?
- [ ] Any single decoration that visibly snaps to a horizontal/vertical pixel grid in a style that calls for hand-placed rotation?
- [ ] Any image flattened with a transparent or black-rectangle artifact?
- [ ] Any "chrome" / "glass" / "3D" treatment that reduces to a flat color shift with no extrusion + highlight + shadow stack?

The agent's job is not done until all of the above are "no."

## 10. The thumbnail test (mandatory)

Shrink the rendered PNG to **220px wide** (App Store search-result thumbnail). Squint. Answer in one sentence:
1. What style is this?
2. What does the app do?
3. Why should I tap it?

If you cannot answer all three in one sentence, the slide failed the thumbnail test. The most common cause is undersized typography or a phone that becomes a tiny rectangle when scaled.
