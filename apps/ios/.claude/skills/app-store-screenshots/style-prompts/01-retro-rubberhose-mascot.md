---
name: retro-rubberhose-mascot
description: 1930s rubber-hose cartoon style — cream/mustard palette, white-gloved mascot, thick black ink outlines, vintage chunky display type, App-Store-Today coziness. Inspired by Cancoco.
inspiration: Cancoco, classic Disney/Fleischer cartoons (Steamboat Willie, Felix the Cat, Bimbo), retro tin-can packaging, 1930s American advertising, modern revival illustrators (Hayden Aube, Pedro Correa)
feel: cozy, friendly, nostalgic, "your daily walk has a tiny cartoon buddy"
---

# Retro Rubberhose Mascot

> **READ FIRST:** [`./_QUALITY_BAR.md`](./_QUALITY_BAR.md) — universal quality rules apply to this style.

## Hard quality rules (this style)

- **Phone size on phone-bearing slides** (this includes the HERO — the hero is NOT exempt): phone height = **80–88%** of canvas height (≈ **2300–2520 px** on a 1320×2868 canvas). The phone bottom MUST bleed off the canvas edge by 4–10% of its own height. **Concretely: render the phone at `height = canvas.height * 0.84` and crop the bottom.** On the hero, the mascot row (4 walking variants) peeks from BEHIND the phone — partially hidden behind its lower edge, not in front of an empty cream slab. Phone smaller than 80% canvas height on the hero is a hard fail.
- **Use the default iPhone bezel.** Use the template's stock `Phone` device frame (the iPhone mockup PNG). Do NOT replace it with a cream paper bezel, a hand-drawn outline, or any custom frame. The retro warmth comes from the cream/mustard *backgrounds, mascot, and inside-phone UI stickers* — not from re-skinning the iPhone itself. A 1-2% warm tint behind the phone (drop shadow color, halo glow) is fine.
- **Mascot scale is non-negotiable**: hero/cover mascot **≥ 420px tall** on a 1320×2868 canvas (≈ 14.5% canvas height). Companion mascots ≥ 300px. A 150px mascot in the corner is a fail.
- **Mascot polish gates** — must hit ALL of these:
  1. **Halftone shadow** on the lower-right third of the body: black dots at 8–14% opacity, dot size 3–4px, denser at edge, fading toward middle. Pure flat fill = clipart.
  2. **Pie-cut eyes ≥ 22% of body width each**, two large overlapping ovals, wedge cut from each, cream `#FBEFD2` whites (not pure white), large solid pupils, tiny cream highlight dot upper-right of pupil.
  3. **Mitten gloves** with 3 black curved cuff lines on each wrist. No cuff lines = no character.
  4. **Sausage-tube arms and legs** — uniform width, no joints. Knees and elbows are forbidden.
  5. **Elliptical ground shadow** under any standing mascot: ~80% body-width wide, 8–10px tall, blurred 6px, `#1A1A1A` at 12% opacity.
  6. **Surface line weight** ≥ 3.5px black ink at this scale, uniform width, `stroke-linecap: round`. Hairlines are a fail.
- The bezel is the default iPhone mockup — see above. (Earlier drafts of this spec required a cream paper bezel; that requirement is rescinded — use the stock bezel.)
- **Paper grain noise on every bg** at 7–9% opacity (SVG `feTurbulence` baseFrequency 0.65, mix-blend multiply).
- **Headline emphasis squiggle**: hand-drawn SVG path, 4–6 control points, 4–5px stroke, accent color (see below), extends 8–16px past the underlined word.
- **Accent color = coral `#E45A4A` by default.** Mustard `#F2BB46` is reserved for **backgrounds**, not accents — mustard-on-cream is ~2:1 contrast (the warm yellows sit at similar lightness as cream/sage) and reads as faded. Coral pops on every bg in this palette.
  - Cream `#F4E6CC` bg → coral `#E45A4A` accent ✅ (~4.6:1)
  - Mustard `#F2BB46` bg → coral `#E45A4A` accent ✅ (~4.0:1) or deep brown `#5C3A1E` ✅
  - Pink `#F3A6B7` bg → coral `#E45A4A` accent ✅ or deep brown `#5C3A1E` ✅
  - Sage mint `#BFD9B6` bg → coral `#E45A4A` accent ✅
  - Cowboy-brown / terracotta bg → cream `#FBEFD2` accent ✅
  - The rule: coral is the universal accent; only swap to deep brown/cream when coral itself clashes with bg-adjacent stickers/illustrations.
- **Sticker UI** inside the phone: every card/button has the 2-3px black border + the hard offset shadow `0 3-4px 0 #1A1A1A`. Soft blur shadows are a fail.

## Vibe summary
This is a warm, hand-drawn world that feels like a vintage tin lunchbox someone forgot in a sunny attic. A chunky yellow can-shaped mascot with white gloves and pie-cut eyes walks you through your day, surrounded by cream-paper backgrounds and chunky brown display type. Every shape is hugged by a fat black ink outline, every color is dialed warm (mustard, peach, pink, mint), and the UI inside the phones is rendered as soft sticker buttons that look pressable. The overall feeling is "1932 cereal box meets 2026 wellness app" — playful, slightly silly, and never cold or corporate.

## Global palette

### Warm cream backgrounds
- Primary cream paper: `#F4E6CC` (slide 1, slide 3 base, slide 5 stat-card)
- Soft butter cream: `#FBEFD2` (lightest variant, inside phone areas)
- Warm off-white: `#FAF3E3` (status bar / card highlights)

### Accent backgrounds (full-bleed slide colors)
- Mustard sun: `#F2BB46` (slide 2 main background, the "16628 steps" slide)
- Mustard deep: `#E5A52E` (gradient bottom edge on slide 2)
- Bubblegum pink: `#F3A6B7` (slide 4 "more you walk" slide background)
- Salmon coral: `#F19A8E` (decorative accents inside pink slide)
- Sage mint: `#BFD9B6` (slide 5 green stats slide)
- Mint deep: `#9CC692` (chart bars, accent shadows on mint)

### Character body fills
- Mustard can-yellow: `#F2BB46` (primary mascot body)
- Peach: `#F2B07A` (secondary mascot body)
- Pink mascot: `#EE92A4`
- Cowboy / wide character body: `#E08767` (terracotta)

### Text & ink
- Outline ink: `#1A1A1A` (a true-warm near-black, never pure `#000`)
- Brown wordmark fill: `#5C3A1E` (the CAN COCO logo color)
- Body text dark: `#2A2118` (slightly desaturated coffee)
- Caption gray: `#6B5E4D` (warm gray for secondary copy)

### UI accents inside phone
- Coupon yellow chip: `#FCD34D`
- Soft red "TODAY" stamp ink: `#C0392B`
- Mint button accent: `#8FCB9B`

## Mascot anatomy & rules

The mascot is the soul of this style. Document every rule strictly.

### Body shape
- A rounded **upright soft rectangle / oval can** — roughly 1.0 wide × 1.25 tall ratio.
- Top: a soft dome (think a can lid with no sharp seam).
- Bottom: identical rounded dome.
- Side silhouette is **slightly convex** — the can bulges out 4-6% in the middle.
- Cowboy / "wide" variant: same height but ~1.4× width, more pumpkin-shaped.

### Fill color
- Default: mustard `#F2BB46`.
- Peach variant: `#F2B07A`.
- Pink variant: `#EE92A4`.
- Body is rendered with a **subtle inner shadow** on the lower-right third — `inset -8px -10px 0 0 rgba(180, 120, 30, 0.18)` equivalent — to suggest 3D roundness without losing flatness.

### Eyes (pie-cut eyes)
- Two large overlapping ovals, roughly 22% of body width each.
- Whites are pure cream `#FBEFD2` (NOT pure white — keep them warm).
- Each eye has a **pie-cut slice** removed (a wedge missing, classic 1930s style), filled with body color OR with the dark pupil itself.
- Pupils: solid `#1A1A1A`, large (about 60% of the eye), positioned to convey expression (looking up-left, down-right, etc.).
- A tiny `#FBEFD2` highlight dot may sit at the upper-right of the pupil (1-2px at full size).
- Eyes are outlined with the same 3-4px black stroke as the body.

### Hands & gloves
- **White four-fingered mitten gloves** — classic rubber-hose hands.
- Glove fill: `#FBEFD2` (warm cream, never pure white).
- Glove outline: same 3-4px black.
- **3-line cuff detail** — three small black curved lines on the wrist of each glove (the classic button-cuff).
- Hand silhouette is a soft pillow oval with a thumb stub and the suggestion of fingers (often just a slight pinch line, not full finger separation).
- Arms are **sausage tubes** — uniform thickness, no elbow joint, no shoulder seam. They emerge directly from the body silhouette.
- Arm width: ~14% of body width.

### Feet / legs
- Same sausage-tube rule: legs are rounded stubs, no knee.
- Shoes: simple rounded **black or brown lozenges** that look like vintage spats or oxfords.
- One foot usually lifted in a walk cycle pose.

### Outline rules
- Every shape gets a **solid black `#1A1A1A` stroke at 3-4px** when the mascot is rendered at ~280-360px tall (on a 1242px phone canvas this is roughly 1% of canvas width).
- Stroke is **uniform width** — no tapering, no calligraphy.
- Stroke style: `stroke-linecap: round; stroke-linejoin: round;`
- NEVER use a thin or hairline outline. If in doubt, make it thicker.

### Pose vocabulary
- **Walking forward**: one arm raised in a wave, one leg lifted, slight forward lean.
- **Holding object**: both gloves cupped in front holding a coin, coupon, or phone.
- **Leaning back / posing**: hand on hip, the cowboy-hatted variant.
- **Tipping hat**: cowboy variant with one glove on hat brim.
- **Headband / sweatband**: fitness variant with a red or pink terry headband.
- **Sitting / chilling**: shorter pose, both legs forward.

### Costume / accessory variations seen
- **Cowboy hat**: brown `#8B5A2B` ten-gallon hat with a darker band.
- **Red sweatband**: thick `#E74C3C` headband across the forehead.
- **Sunglasses**: black rounded squares, no shine.
- **Tiny phone / coin / coupon** held in gloves.

### Surface texture on body
- A very light **halftone dot shadow** sits on the bottom-right curve of the body — small `#1A1A1A` dots at ~8% opacity, arranged in a gradient density (denser at the shadow edge, fading out). This is the 1930s newsprint feel.

## Typography

### Headline display font
- **Cooper Black** (or Recoleta Black, Filson Soft Black, Sofia Pro Black as fallbacks).
- Weight: 900 / Black.
- Letter-spacing: -0.01em.
- Line-height: 1.05.
- Color: dark brown `#2A2118` for body text, or accent color (mustard / red) for the highlighted word.

### Wordmark font (the "CAN COCO" logo)
- A custom-feel chunky slab/sans — closest off-the-shelf match: **Recoleta Black** or **Cooper Std Black** with manual tracking.
- Color: deep brown `#5C3A1E`.
- **Inline white shadow**: each letterform has a thin (2-3px) cream `#FBEFD2` inline highlight offset by `1px 1px` toward the upper-left, sitting INSIDE the letter shape. This creates the classic 1930s "embossed candy bar" look.
- Letter-spacing: 0.02em.
- All-caps.

### Body / UI label font
- **Nunito** or **Fredoka** at weight 600-700.
- Rounded sans, slightly bouncy.
- Color: `#2A2118` for primary, `#6B5E4D` for secondary.

### Caption font
- Same Nunito family, weight 500, size ~11-13px on phone canvas.

### Text effects checklist
- Headline emphasis word: color swap to mustard `#F2BB46` or coral `#F19A8E`, often paired with a hand-drawn squiggle underline in black 3px.
- Wordmark: inline cream shadow as above.
- Sticker labels (App Store badge): cream pill with thin black border.
- NO neon glows. NO gradients on type. NO modern variable-weight tricks.

## Headline emphasis
- One word is colorized — usually the action verb. Examples: in "**Walk** Can be a lot of fun", the word "Walk" is mustard `#F2BB46` while the rest is dark brown `#2A2118`.
- The emphasized word may sit on its own line for visual weight.
- A **hand-drawn underline squiggle** sometimes sits under the emphasized word — 3px black, gently wavy (not perfectly straight).
- Mixing case: headlines are sentence-case ("Walk Can be a lot of fun"), not Title Case or ALL CAPS, except for the wordmark.

## Phone / device frame treatment

- Bezel: **warm cream `#F4E6CC`** for the outer shell, NOT standard black. This makes the phone feel like a tin toy.
- Frame thickness: ~14-18px outer ring.
- Inner ring: a 2-3px `#1A1A1A` black outline traces the outer edge of the phone AND the inner screen edge (because every shape gets the black ink treatment).
- Corner radius: outer 48-56px, inner screen 36-42px (Apple-ish, but exaggerated softness).
- Shadow under phone: a **single soft drop shadow**, warm-toned: `0 24px 40px -8px rgba(92, 58, 30, 0.22)`. Never blue, never pure black.
- Status bar inside phone: simplified — small clock at top-left in dark brown, signal/wifi/battery icons in dark brown, NO color.
- Phone tilt: slides 1 and 3 use **straight upright** phones; slides with the mascot interacting may show a tiny ±2° tilt. Avoid heavy 3D rotation.
- The phone often sits with its **bottom edge cropped off the slide** to feel grounded.

## Background treatment

- Solid warm cream `#F4E6CC` is the workhorse background.
- Accent slides use full-bleed flat color (mustard, pink, mint) — no gradients EXCEPT a very subtle vertical darken at the bottom 25% (e.g., mustard `#F2BB46` → `#E5A52E`).
- **Paper grain overlay**: a 6-8% opacity noise texture across every background. CSS: `background-image: url(noise.png); mix-blend-mode: multiply; opacity: 0.07;`.
- NO photographs as backgrounds. NO gradients with multiple stops. NO blurs.
- Optional: a very faint repeating **halftone dot pattern** at the slide edges, ~4% opacity, dot size 3px, spacing 8px.

## Decorative accents

- **Halftone dot fields**: small black dots on a transparent layer, used to suggest shadow under the mascot or to fill a corner.
- **Scattered ink doodles**: tiny stars (4-point asterisks), squiggles, plus signs, small hearts — all in black 2-3px stroke, sprinkled in negative space at ~6-10% density.
- **Sparkle stars**: 4-point sparkle shapes (like a Twinkle Twinkle Little Star icon) in cream or mustard near the mascot's face to suggest cuteness.
- **Sunbeam rays**: optional — 8-12 thin triangular rays radiating from behind the mascot on accent-color slides, in a slightly lighter tint of the background (mustard rays on mustard at +8% lightness).
- **Footprint trail**: small black footprint icons trailing behind a walking mascot, fading from 100% to 30% opacity along the trail.
- **Polka dots**: large `#FBEFD2` cream dots scattered on pink slide background, 24-40px diameter, very low density.
- **Hand-drawn arrows**: chunky black 4px-stroke arrows with rounded heads, pointing at UI elements.

## Icons & UI sticker treatment

Inside the phone screens, every UI element is a **sticker** with personality.

- **Buttons**: rounded squircle pills, corner radius 18-24px.
- Button border: 2-3px solid `#1A1A1A`.
- Button fill: warm cream `#FBEFD2` for neutral, mustard `#F2BB46` for primary, mint `#9CC692` for success.
- Button drop shadow: `0 3px 0 0 #1A1A1A` (a hard offset shadow that makes it look like a stuck-on sticker), NOT a soft blur.
- Inset highlight: a 1px cream `#FBEFD2` line along the top inside edge of the button.
- Button label: Nunito 700, 14-16px, dark brown.
- Icon style inside buttons: simple line icons, 2.5px stroke, rounded caps, black ink — never filled flat icons.
- Cards: corner radius 20-28px, same 2-3px black border, same hard offset shadow, cream fill with a subtle paper noise.
- The "TODAY" coupon-style stamp uses a red `#C0392B` distressed ink stamp font, slightly rotated -8°.
- Bullet list items in slide 1: each bullet is a small mascot-color filled circle outlined in black, NOT a generic dot.

## Photo / illustration treatment

- **No photos anywhere.** Every visual is flat vector illustration with subtle textural fills.
- Body fills get a **halftone shadow** on the shadow side (described in Mascot anatomy).
- Each character has a **soft elliptical drop shadow** on the ground beneath them — pure `#1A1A1A` at 12% opacity, ellipse ~80% of body width, 8-10px tall, blurred 6px. This grounds the character without breaking the flat aesthetic.
- No gradient meshes, no realistic shading, no Photoshop layer styles. The shading vocabulary is: flat fill + halftone dots + a single mid-tone shape (optional) + black ink outline.
- Illustrated props (coins, hats, coupons) follow the same rules: flat fill, 3-4px black outline, optional halftone shadow.

## Copy tone

- Childlike enthusiasm, second-person, friendly verbs.
- Exclamation marks are welcome but not overused (max one per headline).
- Sentence-case throughout, not Title Case.
- Use simple short words: "Walk", "fun", "cute", "buddy", "today", "steps".
- Quoted lines actually visible on this mosaic:
  - "Walk Can be a lot of fun"
  - "Cute walking buddy"
  - "Step count redeem for cans"
  - "Encountering animals on walks"
  - "Walking Companion"
  - "16628" (large step count)
  - "CAN COCO" (wordmark)
  - "TODAY" (coupon stamp)
  - "The more you walk the more animals you meet"
  - "Accurate Data Recording"
  - "App Store — Featured on Today" (top badge on slide 1)

## Per-slide breakdown (mandatory)

### Slide 1 — "Walk Can be a lot of fun" cover with bullet list
- **Background**: warm cream `#F4E6CC` full bleed with 7% paper noise overlay.
- **Top badge**: an "App Store — Featured on Today" pill at the very top, cream fill, 2px black border, small Apple logo and red "Today" word inside. Centered horizontally, ~60px from top.
- **Headline**: "**Walk** Can be a lot of fun" — large Cooper Black, three lines, left-aligned, starting ~120px from top, left padding ~60px. "Walk" word is mustard `#F2BB46`, rest is dark brown `#2A2118`. Font size ~84-96px on a 1242px canvas.
- **Bullet list**: three rows below the headline.
  - Each bullet: a 24px circle filled in mustard, peach, or pink with a 2.5px black outline.
  - Each label: Nunito 600, ~28-32px, dark brown.
  - Lines: "Cute walking buddy", "Step count redeem for cans", "Encountering animals on walks".
  - Row spacing: ~48px vertical gap.
- **Mascot placement**: a row of 4-5 small mascot variants at the bottom edge of the slide, walking left-to-right, partially cropped at the bottom. Includes the default yellow, peach, cowboy-hat brown, and a sunglasses variant. Each about 180-220px tall.
- **Floating elements**: a few sparkle stars and ink doodles between mascots.
- **Notable effects**: subtle ground shadow under the mascot row; paper grain visible throughout.

### Slide 2 — Mustard "16628 steps" slide with phone & CAN COCO wordmark
- **Background**: full-bleed mustard `#F2BB46` with a vertical gradient darkening to `#E5A52E` at the bottom 25%. 6% paper noise overlay.
- **Headline / wordmark**: "CAN COCO" wordmark in deep brown `#5C3A1E` with cream inline highlight, sitting in the bottom-right area, stacked on two lines ("CAN" / "COCO"), font size ~110-140px, very chunky.
- **Phone**: a single phone centered slightly left, straight upright, cream bezel, showing a screen with a large "16628" step count in dark brown Cooper Black centered, mascot illustration above the count, a "TODAY'S" coupon-style label below in red ink stamp, and a yellow primary action button at the bottom. Phone is roughly 60% of slide height.
- **Mascot placement**: a tiny mustard mascot inside the phone above the step count; also potential walking mascots along the bottom edge outside the phone.
- **Floating elements**: scattered sparkle stars in cream around the wordmark; subtle sunbeam rays behind the phone (mustard +8% lightness).
- **Notable effects**: warm soft drop shadow under the phone; the "TODAY'S" stamp is rotated -6° to feel hand-applied.

### Slide 3 — "Walking Companion" on white/cream
- **Background**: soft butter cream `#FBEFD2` full bleed with 6% noise. A subtle horizon line of slightly darker cream at the lower third.
- **Headline**: "Walking Companion" at the top center, Cooper Black, dark brown, ~72px, single line, with a small mascot-color highlight dot to the left of the word.
- **Phone**: a single phone, upright, cream bezel, showing a "home" screen with three mascot character cards (mustard, peach, cowboy) arranged in a 3-column grid, the big "16628" stat below, and a primary mustard CTA at the bottom labeled with a short action verb.
- **Mascot placement**: two larger mascots OUTSIDE the phone, one on the left (peach, holding a coin) and one on the right (cowboy-hat brown, tipping hat), each about 320px tall, both grounded with elliptical ground shadows.
- **Floating elements**: small footprint trail between the two outside mascots; a couple of ink-doodle stars.
- **Notable effects**: the three card portraits inside the phone each have a 3px black border and a hard offset shadow; subtle halftone shadow on the right side of the outside mascots' bodies.

### Slide 4 — Pink "more you walk, more animals you meet"
- **Background**: full-bleed bubblegum pink `#F3A6B7` with 6% noise. Scattered large cream `#FBEFD2` polka dots at low density (~6 dots, 28-40px each).
- **Headline**: "The more you walk the more animals you meet" at the top, two or three lines, Cooper Black, dark brown, ~58-66px, centered. The phrase "more animals" may be colorized in coral `#F19A8E`.
- **Phone**: a single phone, slight 2° tilt to the right, cream bezel, showing a chat-style screen where the wider pink mascot greets the user with a speech bubble; a mustard CTA at the bottom.
- **Mascot placement**: the **wide pink mascot** (`#EE92A4`, pumpkin-shaped) sits centered inside the phone with arms wide open. A second smaller terracotta mascot (`#E08767`) stands outside the phone on the right at slide bottom.
- **Floating elements**: a few sparkle stars and tiny hearts in cream around the mascots; a soft halftone shadow patch under the phone.
- **Notable effects**: the chat bubble inside the phone has the same 2-3px black border and hard offset shadow as other sticker UI.

### Slide 5 — Mint green "Accurate Data Recording" stats slide
- **Background**: full-bleed sage mint `#BFD9B6` with 6% noise.
- **Headline**: "Accurate Data Recording" at the top, Cooper Black, dark brown, ~64px, with a sparkle icon to the left of "Accurate". "Data" may be colorized in deeper mint `#9CC692` or in mustard `#F2BB46`.
- **Phone**: a single phone, upright, cream bezel, showing a stats / chart screen — a cream `#FBEFD2` card at the top with a small bar chart in mint `#9CC692` bars, a "Today" label, numeric stats in dark brown, and a yellow CTA button at the bottom.
- **Mascot placement**: a tiny mustard mascot **inside the phone** at the top of the stats card, waving. No exterior mascots (or one small one peeking from a corner).
- **Floating elements**: small ink-doodle plus signs and stars around the corners of the slide.
- **Notable effects**: chart bars have the same 2px black outline treatment; the card has a hard offset shadow `0 4px 0 0 #1A1A1A`.

## How to apply this style

1. Lock the palette first: cream `#F4E6CC` for base, mustard `#F2BB46` / pink `#F3A6B7` / mint `#BFD9B6` for accent slides.
2. Apply a 6-8% paper grain noise overlay to every background.
3. Draw or place the mascot at consistent scale (~280-360px tall on a 1242px canvas). Always include the 3-4px black outline and white-gloved hands.
4. Set headlines in Cooper Black (or Recoleta Black), dark brown `#2A2118`, with ONE word colorized in an accent.
5. Render the wordmark in deep brown `#5C3A1E` with a 2-3px cream inline highlight offset to the upper-left.
6. Build phone mockups with **cream bezels**, not black. Add a 2-3px black outline tracing the phone edge.
7. Style all interior UI buttons and cards as **stickers**: rounded corners, 2-3px black border, hard offset shadow `0 3-4px 0 0 #1A1A1A`, soft inset cream highlight on top edge.
8. Sprinkle small ink doodles (stars, sparkles, footprints) at 6-10% density in negative space.
9. Add an elliptical ground shadow under any free-standing mascot at 12% black opacity.
10. NEVER use pure white, pure black, or cool colors as primaries. Everything is warm.

## What this style is NOT

- Do NOT use modern flat geometric icons (no Material symbols, no thin SF icons). Icons must be hand-feel line drawings with 2.5px rounded strokes.
- Do NOT use thin black outlines — outlines must be heavy, 3-4px, uniform.
- Do NOT use cold colors (no #FFFFFF whites, no #000 blacks, no blue/purple gradients) as primaries.
- Do NOT use realistic 3D shading or gradient meshes on the mascot — only flat fill plus halftone dots plus optional single mid-tone.
- Do NOT use sans-serif headlines like Inter, Helvetica, SF Pro. The headline MUST be a chunky retro display.
- Do NOT use Title Case in headlines — sentence-case only ("Walk Can be a lot of fun" is the rule).
- Do NOT use blurred soft shadows alone — sticker UI uses HARD offset shadows.
- Do NOT use photos, photographic backgrounds, photo textures of skin/landscape, or AI-photoreal renders.
- Do NOT use neon, glow, glassmorphism, or any 2020s-era trendy treatments.
- Do NOT separate the mascot's fingers into five — gloves are mitten-style with at most a thumb split.
- Do NOT make the mascot's pupils small dots — they are LARGE, expressive, and pie-cut.
- Do NOT use angular sharp corners on UI — every rectangle is a squircle with 18-28px radii.
- Do NOT use Apple's default black bezels for phone mockups — bezels must be cream.
- Do NOT crop the wordmark or shrink it below readable size; it deserves stage space.
- Do NOT center every headline — slide 1 uses left-aligned text, and slides vary intentionally.
- Do NOT overload a slide with more than ~3 mascots; the mosaic uses small character ensembles only at the bottom of slide 1.
- Do NOT pick saturated or candy-bright colors. The mustard is warm, the pink is dusty, the mint is sage. Everything reads as slightly faded vintage paper.
