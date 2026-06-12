---
name: paper-sticker-skeuomorphic
description: Skeuomorphic paper-craft style — cork-board backgrounds, paper-cutout UI cards, marker handwriting headlines, sticker pixel-art accents, folder tabs. Student-organizer vibe. Inspired by Folderly.
inspiration: Folderly, Tot, Cosmos, paper-craft moodboard apps, 90s school binders, Lisa Frank trapper-keeper, washi-tape journaling, Pinterest scrapbook moodboards
feel: tactile, crafty, "your school binder come to life", joyful nostalgia, dorm-room desk, sticker book, kindergarten art class meets iOS 17
---

# Paper Sticker Skeuomorphic

> **READ FIRST:** [`./_QUALITY_BAR.md`](./_QUALITY_BAR.md) — universal quality rules apply to this style.

## Hard quality rules (this style)

- **Use the default iPhone bezel.** Use the template's stock `Phone` device frame (the iPhone mockup PNG). Do NOT replace it with a paper-cutout or cream cardboard border. The paper-craft world reads through the *background slab*, the *stickers*, the *marker headline*, and the *inside-phone sticker UI* — the iPhone itself stays as the default. You may tape washi-tape strips across the phone corner for the scrapbook feel, but the bezel is real iPhone.
- **Phone size on phone-bearing slides**: phone height = **82–90%** of canvas height (≈ **2350–2580 px** on a 1320×2868 canvas). The phone MUST bleed off the bottom edge by 5–10% of its own height. **Concretely: render the phone at `height = canvas.height * 0.86` and crop the bottom.** Below 82% is a hard fail — the paper-craft world depends on a big anchored phone.
- **Marker headline size**: **≥ 130 px (≈ 96pt)** on a 1320×2868 canvas. Slide 1 wordmark (e.g. "Bloom") **≥ 180 px (≈ 132pt)**. Smaller headlines read as bashful and fail the "binder/sticker book" energy. Use **140–160 px** as your default and only drop if the marker word genuinely cannot fit at 86% canvas width.
- **Marker headline color is bg-dependent.** Marker blue `#3E8FD6` on cork green is medium-on-medium and reads faded.
  - Cork mosaic green bg → **marker red `#E45A4A`** (complementary hue, ~5:1 contrast) — the classic red-marker-on-green-chalkboard look.
  - Peach / pink / dusty rose flat slabs → marker blue `#3E8FD6` ✅
  - Putty grey / mint sherbet → marker blue `#3E8FD6` ✅ or marker red ✅ (either reads well)
  - The squiggle underline follows the headline color exactly.
- **Subtitle size & contrast**: subtitle (the line under the marker headline) MUST be **≥ 44 px (~33pt)** on a 1320×2868 canvas (was 28–32pt — too small to read at thumbnail size on the cork bg). Weight: Inter Semibold 600+. Color: deep navy `#1F2A44` on light slabs; navy stays the same on cork mosaic but consider placing the subtitle on a small cream rounded scrim card if it crosses busy grout — never let small subtitle text float across multi-tone cork.
- **Cork-bg subtitle scrim** (slide 1 only): if the subtitle sits on the cork mosaic, render it on a small cream `#F8F0E2` rounded rect scrim (padding 16px h / 10px v, radius 12px, 1.5px paper shadow). This pins it to readable ground and reads as a "paper label stuck on the cork".
- **Cork tile mosaic on slide 1**: irregular 60–100px tiles, each rotated -2° to +2°, four green hues alternating, 1.5–2px cream `#F4EBDD` grout between tiles, plus 5% brown noise overlay. A flat green field = fail.
- **Marker headline gates** — must hit all:
  1. **Font**: Caveat Brush / Permanent Marker / chunky marker only. Sans-serif = fail.
  2. **Rotation**: -3° to +3° (visible tilt, not axis-aligned).
  3. **Marker blue `#3E8FD6`** (or marker red `#E45A4A` if bg is blue-adjacent).
  4. **Hand-drawn squiggle underline** — 4–6px stroke, 2–3 gentle waves, 8–16px overshoot past the word ends. Straight underline = fail.
  5. **Size**: 72–84pt on 1242px canvas (~96–112px on 1320px canvas).
- **Sticker shadow** on every floating sticker, polaroid, washi-tape: hard offset `0 2px 2px rgba(33,28,22,0.22)`, not a soft blur. Stickers without contact shadow read as floating glitches.
- **Pixel-art stickers**: render at **integer pixel grid**, `image-rendering: pixelated`, with a 2px white "die-cut" outer border. Anti-aliased pixel art = fail.
- **Sticker density**: 5–8 stickers per slide minimum. Below 5 = sparse-maximalist failure.
- **Paper grain noise** at 7–10% opacity, `mix-blend-mode: multiply`. Without it, paper reads as plastic.

## Vibe summary
A joyful, deeply tactile aesthetic that treats every surface as a real, physical piece of paper, cardboard, cork, or vinyl sticker pressed onto a school desk. Each slide is a flat-lay composition: a colored construction-paper or cork background slab, then layered paper-cutout cards (the iPhones themselves are rendered as paper rectangles), with marker-pen headline lettering and a scatter of pixel-art stickers, polka-dot stickers, polaroids, and folder tabs. The vibe sits between a Lisa-Frank trapper keeper, a Pinterest scrapbook, and a Studio Ghibli desk shot — playful, hand-made, joyfully chaotic but compositionally balanced. Every element casts a soft, short paper shadow so the viewer reads depth as "layered paper on a surface" rather than "UI on a screen."

## Global palette

### Background slabs (one per slide)
- Slide 1 — Cork green / mint cork: base `#8FBE7C` with a tiled mosaic of `#A4D08D`, `#7FAE6C`, `#B3D69D`, `#94C283` (irregular ~80×80px tiles), white grout lines `#F4EBDD` at 2px.
- Slide 2 — Peach pink: `#F4C6BB` (warm salmon-blush) with 8% paper grain.
- Slide 3 — Dusty rose / coral pink: `#EFB1A4` (a touch deeper than slide 2) with 8% paper grain.
- Slide 4 — Putty grey / oat: `#C9C5BC` (warm muted greige) with 10% linen grain.
- Slide 5 — Mint sherbet: `#C9E6C2` (pale cool mint, lighter than slide 1 cork) with 8% paper grain.

### Paper white (cards, phones, notes)
- Off-white cream: `#F8F0E2` primary paper color (NEVER pure white).
- Edge highlight: `#FFFAF0` along top-left of each paper edge (1px subtle lift).
- Notepad page: `#FAF3E5` with horizontal rule lines in `#E8DAB7` every 22px.

### Marker / headline color
- Marker blue: `#3E8FD6` (bright but slightly faded, like a Crayola "blue" felt-tip).
- Marker blue shadow / second pass: `#2E76B8` (used as a 1-2px offset behind headline for depth).
- Marker red (accents, occasional words): `#E45A4A`.
- Marker yellow highlighter band: `#FCE36A` at 60% opacity behind some words.

### Text dark
- Body navy: `#1F2A44` (slightly warm, never pure black).
- Secondary body: `#5B6478` (cool grey-blue).
- Hint / placeholder: `#A0A4AE`.

### Sticker accent palette (vinyl stickers)
- Alien green: `#7AD66B` with `#4EA644` shadow.
- Hot pink: `#F25C8E`.
- Retro yellow: `#FBD24B`.
- Sky cyan: `#6CCDE6`.
- Lavender: `#B493DC`.
- Tomato red: `#E45A4A`.
- Sticky-note yellow: `#FAE680` (the classic Post-it shade).
- Washi-tape mint: `#A8E3C8` with white polka dots.

### Folder-tab palette (color-coded courses)
- Tab red `#E96B59`, tab orange `#F2A14B`, tab yellow `#F4D04C`, tab green `#7BC47A`, tab teal `#5FB8C0`, tab blue `#5C9ADF`, tab purple `#A28BD0`, tab pink `#F19BB6`.

### Shadow tokens
- Paper shadow (small): `0 2px 4px rgba(33, 28, 22, 0.10)`.
- Paper shadow (card): `0 4px 8px rgba(33, 28, 22, 0.12), 0 1px 2px rgba(33, 28, 22, 0.08)`.
- Paper shadow (phone): `0 10px 22px rgba(33, 28, 22, 0.18), 0 2px 4px rgba(33, 28, 22, 0.10)`.
- Sticker shadow (vinyl, harder edge): `0 2px 2px rgba(33, 28, 22, 0.22)`.

## Background textures (signature)
Every background is treated as a physical surface, never a flat fill. Layer ordering from bottom up:

1. **Base color slab** — the slide's background color above.
2. **Paper grain noise** — a tileable noise PNG at 7-10% opacity, `mix-blend-mode: multiply`, grain size 1-2px (think construction paper, not photographic film grain).
3. **Subtle vignette** — radial gradient from transparent at center to `rgba(33, 28, 22, 0.06)` at corners, very gentle.

### Per-surface texture rules
- **Slide 1 cork mosaic**: Tile the canvas with irregular 60-100px squares, each rotated `-2°` to `+2°`. Each tile uses one of the four green hues randomly. Between tiles, 1.5-2px gaps in `#F4EBDD` (cream grout). Add 5% additional brown noise (`#6B4F2A` at 5% opacity multiply) so it reads as cork/grass mat, not tile. Tiles closer to edges should be partially cropped so it feels endless.
- **Slide 2 & 3 peach/rose**: Flat construction paper. Add subtle horizontal "fiber" streaks: 1px lines of `rgba(255, 255, 255, 0.05)` every 6-9px irregularly, like recycled paper.
- **Slide 4 putty grey**: Linen weave — two perpendicular sets of 1px lines at `rgba(0, 0, 0, 0.04)` every 3px, creating an extremely subtle crosshatch.
- **Slide 5 mint**: Same construction-paper fiber as slides 2/3 but cooler.

**Every** paper element placed on these backgrounds gets a paper shadow (see tokens). Nothing floats without a shadow. Nothing is on a pure-white sheet.

## Typography

### Headline (the wordmark / slide title)
- Font: chunky hand-drawn marker — Caveat Brush, Permanent Marker, Reenie Beanie Bold, KG Happy Solid Bold, or Sketchnote Square. Strokes ~10-14% of x-height.
- Weight: only one weight (the chunky marker weight).
- Case: Title Case with capitalized first letter, or all-caps for short single words ("Courses", "To-do").
- Color: marker blue `#3E8FD6`.
- Size: 64-84pt at slide width 1242px.
- Letter spacing: -1% to 0%, slightly cramped like hand-lettering.
- Skew/rotation: each headline rotated `-3°` to `+3°` (NOT axis-aligned) to feel hand-written.
- Stroke treatment: optional 1.5px darker outline `#2E76B8` on the lower-right edge of each letter to simulate marker bleed.
- Underline: a hand-drawn squiggle — a wobbly horizontal stroke 4-6px thick in the same marker blue, running under the word, with 2-3 gentle waves. Often extends 8-16px past the word at one or both ends.

### Subtitle / body description
- Font: clean rounded sans-serif — Inter, SF Pro Rounded, or Nunito Semibold.
- Weight: 500-600.
- Color: body navy `#1F2A44`.
- Size: 28-32pt at 1242px width.
- Line-height: 1.35.
- Alignment: left-aligned, max width ~70% of slide.
- Often sits directly under the headline with ~24px gap.

### UI body inside phones
- iOS system / SF Pro Text / Inter at native iPhone sizing (16-17pt).
- Colors and weights follow real iOS conventions inside the app screenshots, BUT every card container is given a cream/paper background instead of pure white.

### Wordmark (slide 1 "Folderly")
- Same marker font as headlines but larger: 110-130pt.
- Same marker blue.
- Slightly heavier squiggle underline (6-8px stroke).
- May include a tiny star sticker (`★` in retro yellow) as a dot on the "i" or floating beside the wordmark.

## Headline emphasis
The headlines themselves ARE the emphasis. There is no separate accent word — the entire headline is the loud, joyful, marker-blue feature. Rules:

- The single noun in quotes ("ID Card", "To-do", "Subto-dos", "Courses", "Folderly") is the headline, marker style.
- Underlined with the squiggle described above. Squiggle color matches the marker headline (blue) UNLESS the slide background is already blue-adjacent, in which case use marker red `#E45A4A`.
- Headline sits in the top ~18% of the slide, left- or center-aligned depending on slide (see per-slide).
- Often slightly off-axis (`-3°` to `+3°` rotation).
- Optional `#FCE36A` highlighter band: a 60% opacity yellow rectangle behind the headline, slightly taller than the cap-height, with hand-drawn wavy top/bottom edges. Used sparingly (1 of 5 slides max).

## Phone / device frame treatment
Phones are NOT real iPhone bezels. They are paper cutouts:

- **Shape**: rounded rectangle, corner radius `48px` at ~620px phone width.
- **Paper border**: 14-18px of cream paper `#F8F0E2` framing the screen, like a paper picture frame. The border is NOT thicker at top/bottom (no notch shelf) — it's even all around.
- **Screen content**: the actual iPhone screenshot fits inside the paper frame, edge-to-edge of the inner rectangle. Inner radius `34px`. No dynamic island or notch — the screen is a clean rounded rectangle.
- **Shadow**: phone paper shadow token (see above). The shadow sits primarily below and slightly right of the phone (light from top-left).
- **Tilt**: phones are rotated `-2°` to `+4°` (slight, never wild). Slide 1 phone is straight or `-1°`; subsequent slides vary.
- **Optional curl**: 1 in 5 phones can have a slight torn or curled top-right corner — a small triangle of paper folded down revealing a darker `#E8DBC2` underside (~30×30px triangle). Use sparingly.
- **Edge highlight**: 1px line of `#FFFAF0` along the top-left edge of the paper border.
- **No status bar**: keep the iOS status bar but recolor it to match the in-app cream theme.

## Background photo / image treatment
There are NO photographs in this style. Every surface is either:

- Flat paper texture (backgrounds).
- Solid color paper cards (phones, notes, ID card body).
- Pixel-art stickers (vinyl cutout look).
- Hand-drawn marker illustrations (squiggles, stars, arrows).

If a real product photo is unavoidable, it MUST be styled as a polaroid: 12px white paper border, 24px chunky bottom border, taped to the surface with two washi-tape strips at the top corners, rotated `-6°` to `+6°`.

## Floating UI elements (stickers, doodles)

### Pixel-art stickers (small, scattered)
All pixel-art stickers are rendered at integer pixel size with crisp `image-rendering: pixelated`. They are vinyl cutouts: matte interior color + 2px white "cut" outline + harder shadow (`0 2px 2px rgba(33,28,22,0.22)`).

- **Alien sticker (Space Invader-style)**: 48×48px, green `#7AD66B` body, white outline, sits near phone bottom on slide 2 and slide 1. Rotation `-12°` to `+8°`.
- **8-bit knight / hero**: 56×56px, grey armor `#B8B6AE` + red plume `#E45A4A`, slide 1 bottom-right of phone. Rotation `+6°`.
- **Retro space invader (pink)**: 44×44px, hot pink `#F25C8E`, slide 1 left side. Rotation `-8°`.
- **Pixel heart**: 24×24px, red `#E45A4A`, scattered.
- **Pixel star**: 28×28px, retro yellow `#FBD24B`, scattered.

### Folder tabs
- Trapezoidal or rectangular tabs sticking out the top edge of a paper card.
- 80-120px wide, 32-40px tall (visible portion).
- Each tab a different folder color (see folder-tab palette).
- 6px corner radius on top corners only.
- 1.5px darker bottom-edge shadow line of same hue at 70% lightness.
- Often 4-6 tabs across the top of a notepad-like card (see slide 5 "Courses").

### Polka-dot stickers
- Circles 14-28px diameter.
- Solid color from sticker palette.
- White 2px outline (vinyl die-cut).
- Scattered in groups of 3-7 in slide corners and around phones.
- Rotation N/A (circles), but offsets are random.

### Washi-tape strips
- 90-130px long, 22-28px tall.
- Translucent pastel color at 75% opacity (mint, pink, yellow).
- Optional pattern overlay: white polka dots, diagonal stripes, or tiny stars.
- Torn/jagged edges on the short ends (irregular hand-cut look — 2-3px serrated edge).
- Rotated `-25°` to `+25°`, placed across corners of polaroids and headlines.

### Polaroid frames
- White paper `#F8F0E2`, 12px border, 24px bottom border.
- Inner image area filled with sticker illustration (no real photos).
- Tape strip across one or both top corners.
- Rotation `-8°` to `+6°`.
- Shadow: card paper shadow token.

### Sticky notes (Post-it)
- Square 100-140px, sticky-note yellow `#FAE680`.
- Slight curl on bottom-right corner — a triangle of `#E6CC54` revealed.
- Tiny handwritten marker text in navy.
- Rotation `-6°` to `+6°`.

### Hand-drawn doodles
- Stars (5-point, drawn with single marker stroke): 20-32px, marker blue or yellow.
- Arrows (curved, with arrowhead): 60-100px, marker blue, 4px stroke.
- Squiggles (3-wave horizontal lines): used as dividers under headlines and beside icons.
- Tiny circles / dots clusters: 4-6 dots in marker red around a point of interest.

### Paper clip
- Silver/grey `#C7CDD5` outlined paper clip, ~36×80px, 3px stroke.
- Pinned at top of a paper card.
- Rotation `+15°` to `+30°`.

### Receipt strip
- Long narrow cream rectangle `#F8F0E2`, ~80px wide × 320px tall.
- Bottom edge scalloped/zig-zag like a torn receipt.
- Monospace text in navy: "Hello, Filomena" or order-receipt content.
- Used as a personalized greeting strip (slide 3 & 4).

## Skeuomorphic UI inside phones
The app UI itself continues the paper aesthetic. Real-iOS conventions are bent to match:

- **Background of every screen**: cream `#F8F0E2` instead of system white.
- **Card containers**: cream rounded rectangles with the paper shadow token. Corner radius 18-22px.
- **List rows**: each row is its own paper "strip" with a tiny 1px paper shadow underneath, separated by 6-8px gaps (NOT divider lines).
- **Folder-tab tops on cards**: many cards have a small colored tab protruding from the top-left, 36-60px wide, like a Manila folder. Tab color = category color.
- **Checkboxes**: hand-drawn squares (3px wobble stroke), checked ones have a crayon-style ✓ in marker blue, slightly off-center.
- **ID card screen**: a paper rectangle with a holographic rainbow gradient stripe diagonally across it — gradient from `#F25C8E` → `#FBD24B` → `#7AD66B` → `#6CCDE6` → `#B493DC` at 30% opacity, plus a 2px white sheen. Photo area is a pixel-art sticker (the alien) rather than a real photo. "Student ID" caption in marker blue at the bottom.
- **Category color coding**: every item (course, todo) carries a 4px colored left edge in its category color, plus a tiny matching color dot.
- **Buttons**: rounded paper rectangles with 8px corner radius, colored fills, sit-on-paper shadow. Tap states slightly recessed (`inset 0 1px 2px rgba(0,0,0,0.1)`).
- **Avatars / profile**: small circular sticker with a pixel-art face inside.
- **Top app bar**: greeting "Hello, Filomena" in semibold navy, subtitle date in lighter grey, no background bar — just floats on cream.

## Decorative accents

- Polka-dot clusters of 3-7 in slide corners.
- Tiny scattered stars (marker yellow + marker blue, drawn 5-point).
- Hand-drawn arrows curving from the headline toward the phone, with arrowhead.
- Washi-tape strips at top corners of polaroids and occasionally taped across the slide background.
- Tape-corner triangles on photo polaroids (matte translucent, jagged edges).
- A single sticker quote bubble (marker outline, white interior) with hand-written text on one slide.
- Scribble underlines, double underlines, and the occasional circled word.

## Copy tone
Casual student / friendly best-friend voice. Conversational, never corporate. Sentences are short and end-stopped. Lowercase-friendly. Uses encouraging verbs ("Personalize", "Create", "Break down", "Choose"). Examples directly from this style:

- "Personalize your own ID Card. Choose from existing logos or upload your own."
- "Create and organize to-dos. Filter them according to Ongoing, Missed and Completed."
- "Break down tasks into smaller, manageable steps, ensuring better organization and progress tracking."
- "Create a course folder to keep all of course-related materials in one place."

Tone notes: never uses exclamation marks aggressively (max 1 per slide). Avoids buzzwords ("seamless", "powerful", "next-gen"). Embraces practical school-life vocabulary ("classes", "homework", "courses", "notes", "lectures").

## Per-slide breakdown (mandatory)

### Slide 1 — "Folderly" Cover
- **Background**: cork-tile mosaic on mint green base `#8FBE7C`. Irregular 60-100px green tiles with cream grout, rotated `-2°` to `+2°` each. Subtle brown noise overlay 5%.
- **Headline / wordmark**: "Folderly" in marker blue `#3E8FD6`, 120pt Caveat Brush style, rotated `-2°`, sits in top-left at about (80px, 80px). Squiggle underline 6px thick extending past the "y". A small yellow star sticker `★` at top-right of the "F".
- **Phone**: centered horizontally, ~620px wide, paper border 16px cream, tilt `0°` (straight). Phone shadow heavy (it's the hero). Screen content shows a To-Do home screen with cream background, "Hello, Filomena" greeting, a small alien sticker avatar, two cards (today's tasks), each with colored category tabs.
- **Floating sticker elements**:
  - Pink retro space invader sticker at top-left of phone (~`-8°` rotation, x=160, y=320).
  - Green alien Space Invader at bottom-right of phone (`+6°` rotation, x=720, y=1500).
  - 8-bit knight sticker at far bottom-left (`+4°`, x=80, y=1900).
  - Three polka-dot stickers (pink, yellow, cyan) bottom-center clustered.
  - Two small marker-blue stars scattered top-right.
- **Notable effects**: cork base reads as a wall pinboard. Phone feels pinned. Slight grain on entire slide.

### Slide 2 — "ID Card"
- **Background**: peach pink `#F4C6BB` flat construction paper with 8% grain and faint horizontal fiber streaks.
- **Headline**: "ID Card" in marker blue `#3E8FD6`, 76pt, rotated `+2°`, top-left at (80px, 80px). Squiggle underline. The quotes-style framing (no actual quotes) — just the words "ID Card" prominently.
- **Subtitle**: "Personalize your own ID Card. Choose from existing logos or upload your own." in navy `#1F2A44`, 30pt Inter Semibold, left-aligned, max-width 720px, sits below headline with 28px gap.
- **Phone**: ~580px wide, paper border 16px cream, tilt `+2°`. Center of slide, slightly lower-half. Screen content shows the in-app ID card editor: a large cream paper card displaying the student ID with the alien sticker photo, holographic rainbow stripe diagonal, "Student ID" label in marker blue, name/ID-number fields, and below it a row of small logo chips (red, mint, lavender) the user can pick.
- **Floating sticker elements**:
  - Tiny pixel alien sticker top-right of the slide (`+10°`).
  - A small "ID" badge sticker in red `#E45A4A` floating beside the phone.
  - 3 polka dots (cyan, yellow, pink) bottom-left cluster.
- **Notable effects**: the ID card on-screen has the rainbow holographic gradient at 30% opacity which is the slide's signature visual moment.

### Slide 3 — "To-do"
- **Background**: dusty rose `#EFB1A4` flat construction paper with 8% grain.
- **Headline**: "To-do" in marker blue, 80pt, rotated `-2°`, top-left. Squiggle underline extending past "o". Optionally a tiny marker-red checkmark doodle next to it.
- **Subtitle**: "Create and organize to-dos. Filter them according to Ongoing, Missed and Completed." Navy, 30pt, below headline.
- **Phone**: ~580px wide, paper border 16px, tilt `+3°`, centered. Screen shows: top bar "Hello, Filomena" + date. Below: pink sticker avatar in a circle. Then a cream card titled "To-do" with a folder-tab top in pink. Card lists 4 tasks each as a paper strip with colored left edge (mint, yellow, red, blue), checkbox at left, task name, due date. Filter chips at top: "Ongoing", "Missed", "Completed" — small paper pill buttons.
- **Floating sticker elements**:
  - Cream receipt strip "Hello, Filomena" peeking from behind top of phone, rotated `-4°`.
  - Two polka dots (yellow, mint) bottom-right.
  - A small washi-tape strip at top-left corner of slide.
- **Notable effects**: paper-strip task rows with category color edges are the readable detail.

### Slide 4 — "Subto-dos"
- **Background**: putty grey `#C9C5BC` with linen crosshatch texture.
- **Headline**: "Subto-dos" in marker blue, 72pt, rotated `+1°`. Squiggle underline. Slightly narrower kerning to fit the longer word.
- **Subtitle**: "Break down tasks into smaller, manageable steps, ensuring better organization and progress tracking." Navy, 30pt, below headline.
- **Phone**: ~580px wide, paper border 16px, tilt `-1°`, centered. Screen shows: same "Hello, Filomena" greeting. A cream card titled "Prepare Research Proposal" with a colored folder-tab top in green. Inside the card: a vertical list of 6 sub-todo rows, each with a hand-drawn checkbox (some checked with crayon ticks in marker blue), small task text in navy, and a 3-dot drag handle on the right. Progress bar at top of card: cream track with marker-blue fill at ~40%.
- **Floating sticker elements**:
  - Receipt strip "Hello, Filomena" again, tucked behind phone top-left.
  - A small yellow star sticker top-right of phone.
  - Polka dots (red, mint, pink) bottom-center cluster.
- **Notable effects**: the checkboxes are visibly hand-drawn (not perfect squares), and crayon ticks are the joyful detail.

### Slide 5 — "Courses"
- **Background**: pale mint `#C9E6C2` flat paper with 8% grain.
- **Headline**: "Courses" in marker blue, 78pt, rotated `-3°`, top-left. Squiggle underline extends well past the "s".
- **Subtitle**: "Create a course folder to keep all of course-related materials in one place." Navy, 30pt.
- **Phone**: ~580px wide, paper border 16px, tilt `+2°`, centered. Screen shows the Courses screen: a large cream notepad card filling most of the screen, with FIVE colored folder tabs sticking out of the top (red, orange, yellow, green, blue from left to right, each ~80px wide). Below the tabs the card body has horizontal ruled lines (notepad style) in `#E8DAB7` every 22px. A title "Courses" inside in marker blue. List of course folder strips below, each a small cream rectangle with a colored tab on its left edge and a course name in navy.
- **Floating sticker elements**:
  - 3 polka dots (pink, yellow, mint) bottom-left cluster.
  - One small pixel star top-right of phone.
  - A small washi-tape strip across the top-right corner of the slide.
- **Notable effects**: the multi-color folder tabs are THE memorable visual — they're the trapper-keeper moment.

## How to apply this style

1. **Pick a slide background color** from the slab palette (mint cork, peach, rose, putty, sherbet). Apply the matching texture overlay (cork mosaic OR construction paper fibers OR linen crosshatch). Always add 7-10% grain noise + subtle corner vignette.
2. **Lay the headline** in marker font (Caveat Brush / Permanent Marker), marker blue `#3E8FD6`, 72-84pt, rotated `-3°` to `+3°`. Draw the hand-drawn squiggle underline by hand or with an SVG path (3 gentle waves, 4-6px stroke).
3. **Write the subtitle** in Inter Semibold navy, 28-32pt, left-aligned, max 70% width, 28px below headline.
4. **Build the phone as a paper cutout**: cream rectangle, 16px paper border, 48px outer radius / 34px inner radius, 0-4° tilt, paper-phone shadow. Place the real iOS screenshot inside the inner rectangle.
5. **Style the in-app UI to match**: cream backgrounds, cards as paper strips with shadows, folder-tab tops on category cards, hand-drawn checkboxes, category color coding via left-edge color bars.
6. **Scatter stickers**: 3-6 stickers per slide. Mix pixel-art stickers, polka dots, washi-tape, and the receipt strip. Each rotated `-15°` to `+15°`. Always with a hard sticker shadow.
7. **Add a hand-drawn doodle accent** if needed: arrow, star cluster, or scribble — never more than 2 per slide.
8. **Audit shadows**: every paper element must have a shadow. No floaters without grounding.
9. **Audit color**: only one bright marker color per slide. Sticker accents can be multi-color but should pull from the palette only.
10. **Verify hand-made-ness**: no axis-aligned headlines, no perfect rectangles for stickers, no pure white anywhere.

## What this style is NOT

- Do NOT use sleek, modern, bezel-less phone frames. Phones must be paper-bordered cutouts.
- Do NOT use thin sans-serif headlines (no Inter Light, no Helvetica Thin). Headlines must be chunky marker handwriting.
- Do NOT use photographs of any kind. Everything is paper, sticker, or pixel art.
- Do NOT use pure white `#FFFFFF` anywhere — always cream `#F8F0E2`.
- Do NOT omit drop shadows from paper cards, phones, or stickers. Shadows are non-negotiable.
- Do NOT axis-align everything. The whole composition should have small 1-3° rotations on most elements.
- Do NOT use gradients on backgrounds. Backgrounds are flat colored paper or cork tiles, never linear/radial gradients.
- Do NOT use neon, glowy, or glassmorphism effects. This is matte paper, not glass.
- Do NOT use corporate or buzzwordy copy. Voice is casual student-friend.
- Do NOT crowd: leave breathing room. Max 6 stickers per slide.
- Do NOT use perfectly geometric checkboxes/arrows/stars inside the UI — they should look hand-drawn.
- Do NOT use system iOS card styles (pure white rounded rect with no shadow). Every card is paper.
- Do NOT mix this style with 3D renders, isometric illustrations, or vector flat-illustration people. Only paper, marker, and pixel art.
- Do NOT use sentence-case marker headlines without rotation — they must visibly tilt to feel hand-placed.
- Do NOT pixel-blur the pixel-art stickers. They must be crisp `image-rendering: pixelated`.
- Do NOT use more than one marker color (blue) for headlines; reserve red sparingly for accents only.
- Do NOT remove the cork tile grout on slide 1 — the grout is what makes it read as cork board, not a green wall.
