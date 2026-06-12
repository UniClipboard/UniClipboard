---
name: hand-drawn-editorial-tasks
description: Deep navy + cream + solid accent slides, with hand-drawn script accent words, tilted phones, doodle squiggles. Productivity-tool design-award vibe. Inspired by Superlist.
inspiration: Superlist, Stamps Camera, GRUG, design-award productivity apps
feel: playful-premium, designer-made, "this team has serious taste"
---

# Hand-Drawn Editorial — Tasks Edition (Superlist canonical)

> **READ FIRST:** [`./_QUALITY_BAR.md`](./_QUALITY_BAR.md) — universal quality rules apply to this style.

## Hard quality rules (this style)

- **Phone size**: 72–82% of canvas height on phone-bearing slides. Tilt the phone within the spec's 10–25° range BUT the bounding box of the tilted phone must still occupy this size range; do not shrink the phone to compensate for tilt.
- **Caveat script accent word**: minimum scale **1.20×** the surrounding sans headline, weight 700, color coral `#F26A50` on cream/navy/purple slides; cream/white on coral slides. Rotation -3° to +5°. Plain italic-sans = fail.
- **Script underline squiggle**: every script-accented phrase gets a hand-drawn coral SVG squiggle below it — 4–6px stroke, 2–3 gentle waves, hand-jittered control points. Smooth perfect curves = fail.
- **Phone shadow** must be warm: `drop-shadow(0 36px 60px rgba(184, 86, 64, 0.30)) drop-shadow(0 4px 12px rgba(0,0,0,0.18))`. Cool/blue shadow = fail.
- **Background is a single solid color per slide**. No gradient. No noise overlay (this style is poster-flat). 3-stop fades = fail.
- **Wordmark "bloom" (or app name) lowercase** in Inter Medium at the top-left corner of every slide, weight 500, 32–40pt. Skipping the wordmark = fail.
- **Doodle accents**: at least 2 hand-drawn SVG elements per slide (squiggle, asterisk, star, curved arrow, scribble underline). Color = coral on light bg, cream on coral bg. Stroke 3–4px, `stroke-linecap: round`. Less than 2 = fail.
- **Closer wordlist** alternates sans + script lines, all centered. Each line scales independently to fit canvas width with 6% horizontal margin. No clipping, no widow lines below 60% width.

## Vibe summary

This is the unmistakable look of a productivity app that hires designers, not just engineers. The deck reads like a magazine spread: solid color blocks per slide, clean lowercase wordmark in the corner, headlines set in a confident medium-weight sans, with ONE accent word per slide rendered in a warm coral hand-drawn script — as if someone took a brush pen to a printed proof. Tilted phones rotated 10–25° float over each block, casting soft warm shadows, while sparse hand-drawn squiggles, stars, and curved arrows orbit the type to break the grid. The contrast is the point: rigid task software made approachable by deliberate, human imperfection. Think Apple Design Award meets coffee-shop poster.

## Global palette

All hex codes:

- **Deep navy** `#1B2336` (primary cover/closer bg variant A), `#18223A` (variant B for dark slides)
- **Cream** `#F5EFDF` (primary alt slide bg), `#EFE8D4` (slightly deeper cream variant)
- **Coral accent** `#F26A50` (script lettering, primary doodle ink), `#E55846` (full coral bg slide), `#FF7A5C` (highlight playhead in phone UI)
- **Electric purple / lavender** `#8B7BFF` (lighter purple slide bg), `#7B5BFF` (mid purple), `#6A4FE5` (deeper purple shade for shadow side)
- **White text on navy** `#FFFFFF`
- **Navy text on cream** `#1B2336`
- **Body subhead grey-blue on navy** `#B8C0D3`
- **Body grey on cream** `#5A6275`
- **Phone dark mode surface** `#0E1018` (deepest), `#161A28` (card surface), `#1F2436` (raised card)
- **Phone divider / hairline** `rgba(255,255,255,0.06)`
- **Pink/coral waveform bars** `#FF7A8C` → `#F26A50` gradient
- **Subtle dark grey for inactive UI** `#2A3148`
- **Award badge gold** `#C9A75A` (cover only, if present)

## Typography

**Primary sans (everything that is not the accent word):**
- Inter Medium / Söhne Buch / Aeonik Medium / GT America Medium
- Tracking slightly tight: `-0.01em` for headlines, `0` for body
- Headlines 60–84pt at canvas scale (sized down on closer wordlist)
- Body / subhead 22–28pt at canvas scale
- Headline color: `#FFFFFF` on dark slides, `#1B2336` on light slides
- Body color: `#B8C0D3` on navy, `#FFFFFF` at 85% on coral/purple, `#5A6275` on cream

**Script accent font (the ONE emphasis phrase per slide):**
- Caveat Bold / Permanent Marker / Reenie Beanie / Homemade Apple / custom hand-lettered SVG
- Preferred: a brush-pen script with visible pressure variation, not a thin monoline
- Size scaled ~115–130% of the surrounding sans (it visually competes with the headline)
- Color: ALWAYS coral `#F26A50` on every background except coral bg itself (on coral bg, the script is `#FFFFFF` or cream `#F5EFDF`)
- Slight rotation: `-3°` to `+5°`, organic
- Baseline shift: `+4px` to `-6px` — let it break the grid

**Wordmark:**
- "Superlist" — clean lowercase wordmark, monoline geometric (looks similar to Inter Medium lowercase), top-left corner
- Color matches the text color of the slide (white on dark, navy on light)
- ~32pt at canvas scale, tracking neutral

**Mix rule:** Sans for the main line, script for the emphasis word. NEVER both styles on the same word. NEVER three font styles on one slide.

## Headline emphasis (signature)

The emphasis phrase is rendered in:
1. Script/handwritten font (brush pen, pressure-varied)
2. Warm coral `#F26A50` (or white on coral bg)
3. Hand-drawn squiggle accompaniment: an underline scribble, a circle-around, a wavy strike, OR a doodle adjacent to it
4. Slight rotation `-3°` to `+5°`
5. Baseline broken — the script word does NOT sit on the sans baseline; it floats `±4–8px` off

**Examples observed in the mosaic (replicate verbatim):**

- Slide 1 (cover, navy bg): `The one app that fits` (sans, white) + new line + `your whole day` (script, coral, with a wavy hand-drawn underline curving beneath the descender of "y")
- Slide 2 (purple bg): `Express` (script, coral, top right area as an over-word) layered over sans-set body — OR the headline is purely sans and the script appears as a side annotation. If unsure, render the accent word that follows the verb in script coral.
- Slide 3 (cream bg): `Tasks and notes` (sans, navy) + `together.` (script, coral, with squiggle line under it) + new line + `Turn your thoughts into actions.` (sans regular, navy, smaller)
- Slide 4 (coral bg): `Talk tasks with AI` — "AI" (or "Talk") rendered in script white/cream against coral, with a starburst doodle nearby
- Slide 5 (cream bg, wordlist): every other entry in script-coral, alternating with sans-navy (see closer section)

Document the rule: **exactly one script phrase per slide.** Never two.

## Phone / device frame treatment

- Slim modern iPhone bezel, ~5–6px black frame at canvas scale
- Screen corner radius matches device (~58px on the iPhone 15-class frame)
- **TILT IS MANDATORY.** Every phone rotates between `10°` and `25°` off vertical.
  - Slide 1 phone: `-12°` (leans left) on the navy cover
  - Slide 2 phone: `+15°` (leans right) on purple
  - Slide 3 phone: `-10°` (leans left) on cream
  - Slide 4 phone: `+18°` (leans right) on coral
  - Slide 5: no phone
- **Soft drop shadow with warm tint:**
  - On navy bg: `0 30px 60px rgba(8, 12, 30, 0.55), 0 8px 20px rgba(0,0,0,0.35)`
  - On coral bg: `0 24px 50px rgba(120, 28, 14, 0.35), 0 6px 14px rgba(60,10,5,0.25)`
  - On purple bg: `0 28px 55px rgba(40, 24, 110, 0.40)`
  - On cream bg: `0 30px 55px rgba(40, 35, 25, 0.18), 0 6px 14px rgba(40,35,25,0.10)`
- **Phone always shows DARK MODE UI** (`#0E1018` background) regardless of slide bg color
- **Phone may bleed off the bottom edge.** On every slide except 1, the phone is cropped at the canvas bottom by 15–30% of its height — never show the full device.
- Phone width occupies ~55–70% of slide width when measured along its tilted bounding box.
- Status bar visible at top of phone, time `9:41`, full battery, full signal — standard Apple promo state.

## Background treatment per slide

Each slide is one solid color block, edge-to-edge, no gradients (except a near-imperceptible vignette).

- **Slide 1 (cover):** navy `#1B2336`
- **Slide 2:** lavender-purple `#8B7BFF` (mid-tone, slightly desaturated)
- **Slide 3:** cream `#F5EFDF`
- **Slide 4:** coral `#E55846`
- **Slide 5 (closer):** cream `#F5EFDF` (matches slide 3, intentional bookend)

**Texture overlay:** apply `~3% noise grain` (monochromatic) across each bg to add a printed-poster feel. SVG `<feTurbulence baseFrequency="0.9" numOctaves="2"/>` at very low opacity, or a PNG noise tile at `opacity: 0.04`.

**Optional vignette:** `radial-gradient(circle at 50% 60%, transparent 60%, rgba(0,0,0,0.08) 100%)` — extremely subtle, only on dark slides.

## Hand-drawn doodle accents (signature, mandatory)

Doodles are the soul of this style. Without them it collapses into generic minimal.

**Types (use 2–4 per slide, varied):**
- Scribbled wavy lines (sine-wave-ish but uneven), 2–5 per slide
- 5-point stars and asterisks (3–6 pointed)
- Curved arrows pointing AT phone features (with little arrowhead)
- Cloud-like blob squiggles (3–4 loops linked)
- Spiral swirls (1.5 to 3 turns)
- Circle-around-a-word marks (incomplete loop, ends not meeting)
- Squiggly underlines beneath the script word (mandatory under at least slide 1's emphasis)
- Short hatch marks / triple slashes `///` for emphasis

**Stroke:**
- Width ~4–6px at canvas scale (assume 1284×2778 canvas)
- Use `stroke-linecap: round` and `stroke-linejoin: round`
- Slight pressure variation: vary stroke width along the path by ±20% (use SVG `<path>` with width modulation, or layer two paths at slightly different opacities)
- Color: coral `#F26A50` on navy/cream bgs; white `#FFFFFF` or cream `#F5EFDF` on coral/purple bgs

**Rotation & placement:**
- Organic. Doodles rotate at any angle.
- Never centered behind the phone — they orbit margins: top corners, between headline and phone, beside the wordmark, off the phone edge as if pointing in.
- One doodle per slide may overlap the phone bezel for depth.

**Count per slide observed:**
- Slide 1: ~3 doodles (wavy line under "your whole day", a small star top-right area, a curl/squiggle near the top-right corner)
- Slide 2: ~4 doodles (cluster of squiggles top-right in coral/white, a small swirl near phone, a star)
- Slide 3: ~2 doodles (underline under "together", a small mark near phone)
- Slide 4: ~3 doodles (star burst near "AI", squiggle top, arrow toward phone)
- Slide 5: ~1–2 doodles (small accent near wordmark area)

## Decorative accents (additional)

- Apple Design Award / Webby badge: optional on cover only, bottom-center or bottom-right, small (~120px wide), gold or muted color so it doesn't fight the script.
- "Editor's Choice" ribbon: optional, never on more than one slide.
- No emoji. No stock icons. Doodles + tilted phones carry the deck.

## Phone UI inside (dark mode tasks)

The phone screens are themselves designed with care. Match these patterns:

- **Background:** deep navy `#0E1018`, no gradient
- **Top app bar:** small lowercase wordmark or page title in white `#FFFFFF`, tracking-tight, ~20pt
- **Cards:** rounded corners `16–20px`, surface `#161A28`, hairline border `1px rgba(255,255,255,0.06)`, internal padding `20px`
- **Leadership-sync card (slide 3):** title "Leadership Sync" in white medium, subtitle in `#7C8597` grey, attendees row with overlapping circular avatars, agenda checklist with rounded square checkboxes (`6px` radius), unchecked = hollow `1.5px` border in `#3A4258`, checked = filled coral `#F26A50` with white check
- **Waveform UI (slide 2):** horizontal row of vertical rounded bars varying height, gradient pink→coral `#FF7A8C → #F26A50`, ~24–32 bars, with a circular playhead dot in bright coral overlaid on one bar; time stamps below in mono grey `#8089A0`
- **AI/voice UI (slide 4):** chat-style bubbles with translucent dark surface, a coral mic button at bottom, possibly a coral waveform indicating recording
- **Task list (slide 1 phone):** vertical list of tasks with rounded checkboxes, item title in white `#FFFFFF`, due date in grey `#7C8597`, swipe-style action chips peeking from right edge
- **Search field:** translucent surface `rgba(255,255,255,0.06)`, rounded `12px`, magnifier icon `#7C8597`, placeholder "Search" in same grey
- **Reactions / emoji pills:** small rounded pills on messages showing `👍 3`, `❤️ 2` — coral accent on the count
- **Right-side floating sticker cards:** detached mini-cards (Slack-style reactions, pinned items, "Just now" notification toasts) floating just off the phone screen edge — they sit on the slide bg, not inside the phone, with their own small shadow. These create a 3D collage effect.

## Closer slide (feature wordlist) format

This is slide 5, the signature payoff slide.

**Layout:**
- Cream `#F5EFDF` background
- Left-aligned vertical stack of feature names, top to bottom
- Generous line-height (`1.15`)
- Stack starts ~12% from top, ends ~10% from bottom
- Margin left ~8% of canvas width

**Pattern (every other entry in script+coral, the rest in navy sans):**

```
Real-time            ← script coral
collaboration        ← sans navy
Repeatable           ← script coral
tasks                ← sans navy
Offline              ← script coral
support              ← sans navy
Widgets              ← script coral
Nested               ← script coral
Lists & Tasks        ← sans navy
Multiplatform        ← sans navy
Integrations         ← script coral
Reminders            ← sans navy
AI-Powered           ← script coral
Privacy              ← script coral
First                ← sans navy
Fast AF              ← script coral
```

**Mixed sizes for visual rhythm:**
- Script entries range 56–82pt
- Sans entries range 40–60pt
- Sizes alternate large/small to create a typographic poster rhythm
- Each entry may rotate `-2°` to `+3°` independently
- Some entries may be slightly indented (0–60px) to break the left rag

**Right edge:** entries may bleed off the right edge of the slide — that is intentional. The list feels like it continues forever.

## Copy tone

Conversational, playful, slightly bold. Quote actual lines:

- "The one app that fits your whole day"
- "From quick thoughts on the go to big team projects at work and even your weekend plans, Superlist keeps it all together." (subhead body on cover)
- "Tasks and notes together. Turn your thoughts into actions."
- "Talk tasks with AI. Use your voice to create lists and tasks."
- "Real-time collaboration", "Repeatable tasks", "Offline support", "Widgets", "Nested Lists & Tasks", "Multiplatform", "Integrations", "Reminders", "AI-Powered", "Privacy First", "Fast AF"

**Rules:**
- Sentences can break grammar (fragments OK: "Widgets.", "Fast AF.")
- Lowercase OK in body, sentence case in headlines
- No exclamation marks
- No emoji in copy
- A little cheeky ("Fast AF") is on-brand
- Never use marketing-speak like "Revolutionary" or "Game-changing"
- "AI" is fine, even celebrated, but not the dominant message

## Per-slide breakdown (mandatory)

### Slide 1 — Cover ("The one app that fits your whole day")
- **Background:** navy `#1B2336`, 3% noise overlay
- **Wordmark:** "Superlist" top-left, white `#FFFFFF`, ~32pt lowercase
- **Headline:** stacked, left-aligned, starts ~18% from top
  - Line 1: "The one" (sans white, ~76pt)
  - Line 2: "app that fits" (sans white, ~76pt)
  - Line 3: "your whole day" (script coral `#F26A50`, ~84pt, rotated `+3°`, with a wavy hand-drawn underline in coral curving beneath, the underline stroke ~5px round-cap)
- **Body subhead:** below headline, ~24pt, color `#B8C0D3`, max width 60% of slide, line-height 1.4. Text: "From quick thoughts on the go to big team projects at work and even your weekend plans, Superlist keeps it all together."
- **Doodles:**
  - Wavy underline under "your whole day" (coral, ~5px stroke)
  - Small 5-point star top-right area (coral, ~40px)
  - Short squiggle/curl near upper-right corner (coral, ~3 loops)
- **Phone:** tilt `-12°` (leans left), positioned bottom-right, bleeds off bottom edge ~25%, shows task list UI with checkboxes and items in dark mode
- **Floating elements:** none on cover — keep cover clean
- **Notable effects:** soft warm shadow under phone; tiny gold Apple Design Award badge optional bottom-center

### Slide 2 — Express / Voice ("waveform")
- **Background:** lavender-purple `#8B7BFF`, 3% noise overlay
- **Wordmark:** "Superlist" top-left, white `#FFFFFF` (or could be omitted on internal slides)
- **Headline:** top area, left-aligned. Could read "Express yourself" or similar — with the verb in sans white and the object/emphasis in script coral `#F26A50` rotated `+2°`
- **Doodles:**
  - Cluster of squiggles top-right in cream/white (3–4 lines)
  - Small swirl near top-left of phone (white, ~3 turns)
  - A 5-point star upper area (coral)
  - Optional curved arrow pointing to the waveform on the phone
- **Phone:** tilt `+15°` (leans right), centered-low, bleeds off bottom edge ~20%, shows pink/coral waveform UI with vertical rounded bars and a playhead dot
- **Floating elements:** a small floating reaction or note card may peek off the right edge of the phone, with its own shadow
- **Notable effects:** the coral waveform inside the dark phone is the visual hook — make sure it pops against the purple bg through the phone bezel

### Slide 3 — Tasks and notes together
- **Background:** cream `#F5EFDF`, 3% noise overlay
- **Wordmark:** "Superlist" top-left, navy `#1B2336`
- **Headline:** top-center-left, ~14% from top
  - Line 1: "Tasks and notes" (sans navy, ~72pt)
  - Line 2: "together." (script coral `#F26A50`, ~80pt, rotated `-2°`, with a single squiggly underline beneath in coral ~5px)
  - Sub-line: "Turn your thoughts into actions." (sans navy regular, ~28pt, color `#5A6275`)
- **Doodles:**
  - Squiggly underline under "together"
  - One small accent doodle (asterisk or short curl) near the phone edge in coral
- **Phone:** tilt `-10°` (leans left), positioned center-bottom, bleeds off bottom ~25%, shows Leadership Sync card with attendees, agenda checklist with coral-filled checkboxes, possibly a green-leaf nature photo card above the meeting card
- **Floating elements:** none required, but a small "✓ Just now" toast card may float off the phone's right edge
- **Notable effects:** cream bg makes the coral script the loudest element — keep navy headline calm to let the script sing

### Slide 4 — Talk tasks with AI
- **Background:** coral `#E55846`, 3% noise overlay
- **Wordmark:** "Superlist" top-left, white `#FFFFFF`
- **Headline:** top, left-aligned
  - Line 1: "Talk tasks with" (sans white, ~72pt) + "AI" (script cream/white, ~90pt, rotated `+5°`)
  - Sub-line: "Use your voice to create lists and tasks." (sans white at 90%, ~28pt)
- **Doodles:**
  - Starburst / asterisk near "AI" in white (~60px)
  - Squiggle across the top of the slide in white (~5 loops, ~5px stroke)
  - Curved arrow in white pointing from headline area toward the phone mic button
- **Phone:** tilt `+18°` (leans right), positioned center-bottom, bleeds off bottom ~25%, shows the AI/voice UI: chat bubbles, coral mic button, possibly a coral waveform indicating active recording
- **Floating elements:** a small AI-suggestion card may float off the phone's top-right edge
- **Notable effects:** doodles flip to white on this bg — coral script would disappear into the coral bg, so the rule inverts. Phone shadow uses deeper warm red tone.

### Slide 5 — Feature wordlist closer
- **Background:** cream `#F5EFDF`, 3% noise overlay
- **Wordmark:** "Superlist" top-left, navy `#1B2336` (or omit since the list is the whole slide)
- **Headline:** the wordlist IS the headline. No separate title needed.
- **Content:** vertical stack of feature names as specified in "Closer slide (feature wordlist) format" above — alternating script-coral and sans-navy, mixed sizes 40–82pt, slight rotations, some bleeding off right edge
- **Doodles:** 1–2 minimal — perhaps a small star or curl somewhere in the lower-right negative space; do not over-decorate, the type IS the decoration
- **Phone:** NONE on this slide
- **Floating elements:** none
- **Notable effects:** this slide is the typographic exclamation point of the deck. Treat like a poster spread.

## How to apply this style

1. Set the canvas background per slide using the listed solid colors in order (navy → purple → cream → coral → cream).
2. Add the 3% noise overlay layer to every slide.
3. Place the lowercase "Superlist" wordmark top-left of each slide in the appropriate text color.
4. Set the headline in clean medium-weight sans (Inter Medium / Söhne Buch / Aeonik Medium), color per slide bg.
5. Identify exactly ONE phrase per headline to render in the hand-drawn script font (Caveat Bold / Permanent Marker / hand-lettered SVG), colored coral `#F26A50` on navy/cream/purple slides, and white/cream on the coral slide.
6. Rotate that script phrase `-3°` to `+5°` and shift its baseline `±4–8px` from the sans baseline.
7. Add a hand-drawn squiggle, underline, circle, or arrow accompanying the script word (mandatory on at least 3 of 5 slides).
8. Add 2–4 additional doodles per slide in the appropriate ink color (coral on navy/cream, white on coral/purple). Stroke ~4–6px, round caps, slight pressure variation.
9. Render the phone in a slim modern iPhone frame, dark mode UI inside (`#0E1018`), tilted `10–25°` per the per-slide spec, with a warm-tinted soft shadow.
10. Bleed the phone off the bottom of the canvas by 15–30% on every slide that has one.
11. Add 0–2 floating sticker/reaction/toast cards beside the phone edge for depth.
12. For the closer slide: skip the phone, build the vertical wordlist with alternating script-coral and sans-navy entries at mixed sizes, individual rotations, and right-edge bleed.
13. Audit the deck: every slide must have (a) the wordmark, (b) the solid bg, (c) one script-coral phrase, (d) at least one doodle, (e) one tilted phone (except slide 5).
14. Quote the actual copy lines verbatim where possible; if the app is not Superlist, adapt copy to match the playful-fragment tone ("Fast AF.", "Widgets.").

## What this style is NOT

- Do NOT center phones upright — they MUST tilt `10–25°`. An untilted phone breaks the entire style.
- Do NOT use sans for the emphasis word — script is the move, every time.
- Do NOT skip the doodles. A slide with no squiggles falls flat — minimum 2 per slide.
- Do NOT use a flat white background — backgrounds must be solid navy, coral, cream, or purple per slide.
- Do NOT use gradients on the slide background. Solid color blocks only (with optional 3% noise).
- Do NOT render the phone in light mode — phones are always dark mode regardless of slide bg.
- Do NOT use serif fonts anywhere. Sans + script is the entire typographic system.
- Do NOT use more than one script phrase per slide. The rarity is what makes it land.
- Do NOT use thin monoline script fonts (e.g. Pacifico Light) — the script must be a brush pen with pressure variation.
- Do NOT use coral script on a coral background — invert to white/cream when on the coral slide.
- Do NOT render full phones — they must bleed off the bottom edge of the canvas.
- Do NOT use emojis in copy. Doodles replace emojis entirely.
- Do NOT add stock UI icons floating around the slide. The hand-drawn doodles are the only ornamentation.
- Do NOT use bright pure-saturation colors (#FF0000, etc.) — the coral, purple, cream are all slightly muted/earthy for a printed-poster feel.
- Do NOT center-align the headline. Left-aligned with intentional ragged right is the rhythm.
- Do NOT use ALL CAPS for headlines. Sentence case only.
- Do NOT add a CTA button or "Download now" — this deck sells through taste, not asks.
- Do NOT make the doodles look digital/vector-perfect — they must read as drawn-by-hand with imperfect loops and uneven strokes.
