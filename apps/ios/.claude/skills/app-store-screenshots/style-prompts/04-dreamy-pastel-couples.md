---
name: dreamy-pastel-couples
description: Dreamy cotton-candy sky gradient with kawaii pets and 3D globes. Italic serif emphasis word in lilac/coral. Soft lavender hearts and chat-bubble floaters. Inspired by Between / Couple apps.
inspiration: Between, Couple, Together couples apps, Korean/Japanese romance app aesthetics
feel: tender, dreamy, kawaii-romantic, "your shared little world"
---

# Dreamy Pastel Couples

> **READ FIRST:** [`./_QUALITY_BAR.md`](./_QUALITY_BAR.md) — universal quality rules apply to this style.

## Hard quality rules (this style)

- **Phone size**: 70–80% of canvas height on phone-bearing slides. Below 70% reads as empty.
- **Italic-serif emphasis word is non-negotiable, AND it must be readable.** On every slide with a headline, exactly one phrase rendered in: (a) a serif italic family (Playfair Display Italic / Newsreader Italic / Tiempos Italic), (b) **deep violet `#5B3FC8`** (occasionally rose `#C25A6E`) — NOT pale lilac `#B49BE6` (which fails WCAG AA on every slide bg in this style), (c) 1.05–1.10× scale relative to surrounding sans, (d) NOT bold. The pale lilac was failing thumbnail-size legibility; the deep violet still reads as romantic + serif but actually achieves ~6:1 contrast against the cotton-candy gradient and cream backgrounds. If the emphasis word feels like it's disappearing, the contrast has failed — re-render.
- **Headline must NOT overlap the 3D globe AND must be center-aligned.** The headline is always horizontally centered across the canvas — never left-aligned. To preserve both rules, shrink the globe to ≤ 22% canvas width and pin it to the upper-right corner (top: ~2% canvas height, right: ~3% canvas width). The centered headline at ~85% canvas width should never touch the globe — verify the rightmost letter of the longest line ends at least 60px before the globe's left edge.
- **3D globe quality**: NOT a flat circle with a hue gradient. Required: (a) ocean fill `#BAD4F2` base, (b) mint sherbet `#BDE3C9` landmass shapes drawn on top, (c) terminator-shadow darker mint `#9CCDB1` on the shaded hemisphere, (d) a warm white `rgba(255,255,255,0.40)` 30px-blur specular highlight upper-left, (e) navy drop shadow `0 24px 40px rgba(27,34,64,0.18)`. Minimum diameter 460px on this canvas. A flat 2-color disc = fail.
- **Kawaii mascot quality**: must have **face features ≥ 30px each** at the rendered scale (cheek blush, eyes with highlights, a mouth, ears or steam puff). A featureless silhouette = fail.
- **Cotton-candy gradient**: full 3-stop `linear-gradient(180deg, #DDEBFA 0%, #F5E0F0 45%, #FCEFD6 100%)` OR a sanctioned per-slide alt (butter yellow / peach pink). A 2-stop near-identical gradient kills the dreaminess.
- **Decoration density**: 3–5 elements per slide (cloud + heart + chat bubble + stamp + sparkle). Below 3 reads as bare; above 7 reads as chaos.
- **Use the default iPhone bezel.** Use the template's stock `Phone` device frame (the iPhone mockup PNG). Add a soft navy drop-shadow `drop-shadow(0 24px 40px rgba(27,34,64,0.18))` around it, but do NOT strip the bezel. The dream comes from the cotton-candy bg + kawaii mascot + lilac italic emphasis — not from skinning the phone.

## Vibe summary
This is the visual language of a quiet love letter rendered as a screen. Each slide feels like the inside of a shared diary written in pastel highlighter — cotton-candy skies, hand-shaped 3D globes the color of mint ice cream, and chubby kawaii pets that look like plush toys. The mood is unmistakably Korean/Japanese romance-app: tender, intimate, second-person, and obsessed with tiny gestures (a heart counter, a chat bubble, a paw print on a map). The single most identifiable signature is the italic serif emphasis word in lilac dropped into an otherwise clean sans-serif headline — that ONE word does all the emotional heavy lifting and must be present on every slide that has a title.

## Global palette

### Sky / background gradient stops
- Sky top (cool blue): `#DDEBFA`
- Sky mid (lilac haze): `#F5E0F0`
- Sky bottom (peach cream): `#FCEFD6`
- Alt background — pastel butter yellow (Raising Pets slide map): `#FFF8D4`
- Alt background — peach pink (Miss You slide): `#FBE3D7` to `#F7C9D6`

### Text colors
- Primary headline: deep navy `#1B2240`
- Emphasis word (italic serif): lilac/violet `#B49BE6`
- Alternate emphasis (coral, allowed once per mosaic): `#F39A9A`
- Subtitle / muted: dusty grey `#8A8FA3`
- In-phone UI body: charcoal `#2A2E3D`
- Wordmark microtype: `#A7B0C4` at 70% opacity

### Globe colors
- Ocean / sea: pale baby blue `#BAD4F2`
- Land mass: mint sherbet `#BDE3C9`
- Land mass shadow (terminator side): muted sage `#9CCDB1`
- Highlight on top-left of sphere (light source): warm white `#FFFFFF` at 40% with 30px blur
- Drop shadow under globe: `#1B2240` at 12% opacity, 40px blur, y-offset 24px

### Sticker / accent colors
- Lavender chat bubble fill: `#B49BE6`
- Lavender chat bubble shadow: `#7E66B5` at 22%
- Hot pink heart sticker: `#FF6FA3` (with neon glow `#FF6FA3` at 50% opacity, 28px blur)
- Soft pink heart card fill: `#FFD7E1`
- Buttercup yellow sticker: `#FFE27A`
- Mint stamp accent: `#BDE3C9`
- White card fills: pure `#FFFFFF` with `#1B2240` 8% drop shadow

### Pet illustration palette
- Husky grey: `#C9D2DA` body, `#FFFFFF` chest, `#3A4154` nose
- Cinnamon-brown pup (Shiba-style): `#E8A86A` body, `#FFE9CC` muzzle
- Panda monochrome: `#FFFFFF` body, `#2A2E3D` patches
- Smiling cream cat: `#F4D9B0` body, `#FFE8C9` belly
- Universal pet blush: `#F8B6C5` at 60%
- Pet eye dots: `#1B2240` with `#FFFFFF` catchlight

## Gradient & atmospheric treatment
The dominant background is a smooth vertical 3-stop gradient: cool sky-blue at the top, transitioning through a lilac haze in the middle, into a peach cream at the bottom. The gradient runs strictly 180deg (top to bottom). Stops sit roughly at 0% (`#DDEBFA`), 55% (`#F5E0F0`), 100% (`#FCEFD6`).

Layer a subtle film grain on top: 1.5% monochrome noise at `mix-blend-mode: overlay` — just enough to break up banding.

Cloud overlay: very faint cotton-puff cloud illustrations (soft white shapes, 8–12% opacity, gaussian blur 6px) drifting horizontally near the top third of each slide. Clouds are oblong, rounded, no outlines — think drawn-with-a-cotton-ball. Never let clouds become a focal element; they're atmosphere.

The peach/pink slide ("Miss You") uses a warmer rotation of the same gradient: `#FBE3D7` → `#F7C9D6` → `#F3B6CC`. Same noise, same cloud treatment.

The pets slide swaps the sky for a flat butter-yellow `#FFF8D4` with a stylized cream-and-mint map overlay (see Map treatment).

## Typography

### Font stack
- Headlines (sans): Pretendard, Apple SD Gothic Neo, Inter, system-ui — weight 600 (Semibold), letter-spacing -0.01em
- Emphasis word (serif italic): Newsreader Italic, Tiempos Italic, Source Serif Pro Italic — weight 400, italic, letter-spacing -0.005em
- Subtitle: same sans stack, weight 500, color dusty grey `#8A8FA3`
- In-phone body: Pretendard Regular (or Noto Sans KR fallback)
- Microcopy under headline (cover wordmark "Couples Use"): tracking +0.12em, ALL CAPS, weight 500, 11px

### Headline size hierarchy
- Cover headline (3 lines): 56–64px, line-height 1.05
- Interior slide headlines (2 lines): 40–46px, line-height 1.1
- Subtitle: 18–20px, line-height 1.4
- Microcopy: 11–13px

### Title case rules
Headlines use Title Case (every major word capitalized). Subtitles use sentence case. Quoted phrases inside headlines wrap in straight double quotes `"Miss You"`.

## Headline emphasis (signature)
Exactly ONE word per headline receives the dual transformation: it switches to **italic serif** AND changes color to **lilac `#B49BE6`**. This word is almost always an adjective ("exclusive", "real", "together") or a verb that carries the emotional payload ("connect", "miss", "share").

Examples:
- "The *exclusive* app for couples." → "exclusive" in italic lilac
- "Connect With *Your* Lover" → "Your" in italic lilac (rendering choice — pick the word that carries possessiveness)
- "*Real* Time Location Sharing" → "Real" in italic lilac
- "Raising Pets *Together*" → "Together" in italic lilac
- "Send A Sw *'Miss You'*" → the quoted "Miss You" phrase in italic lilac

The italic word is rendered slightly larger (1.05x of the surrounding sans) and has a tiny negative letter-spacing (-0.01em) to feel like a handwritten flourish. No underline. No shadow. Color alone + italic + serif does the work.

## Phone / device frame treatment
Slim iPhone 14/15 frame — black titanium bezel (`#1B1B1F`), 3px even border, 48px corner radius, no Dynamic Island detail visible (or a tiny pill dock). Phones are upright, dead center horizontally on their slide. Drop shadow: `#1B2240` at 14% opacity, 40px blur, y-offset 16px — soft and floaty, never crisp.

Phone scale: phone is approximately 62% of the slide width on phone-hero slides, 48% on globe-hero slides where the globe overlaps in front of (or behind) it.

On the cover slide and "Connect With Your Lover" slide, the 3D globe sits LARGER than the phone and visually dominates — the phone effectively disappears or is replaced by the globe as the hero. On the cover, there may be no phone at all — just the globe with floating UI fragments and avatar pins.

## 3D Globe (signature element on first 3 slides)

The globe is the visual anchor of the brand. Specs:

- **Geometry**: perfectly spherical, 3D-rendered with very soft Lambertian shading. Diameter is roughly 1.3–1.5x the width of the phone (or ~52% of slide width if no phone). Sits center-frame, slightly below vertical center on cover, lower-left on "Connect With Your Lover" slide.
- **Surface**: stylized continents in mint sherbet `#BDE3C9` over baby-blue oceans `#BAD4F2`. Continent edges are slightly rounded — no jagged coastlines. No country borders, no labels, no latitude lines.
- **Lighting**: single warm-white light from top-left at ~45deg. Highlight is a soft white smudge at upper-left covering ~18% of the sphere, blurred 30px. Terminator (shadow side) on lower-right is a 12% navy gradient that wraps the sphere edge.
- **Ground shadow**: a soft ellipse `#1B2240` at 10% opacity, 60px blur, sits directly under the globe — never extends beyond globe diameter.
- **Journey line**: hand-drawn dotted line in white (`#FFFFFF` 80% opacity) connecting two avatar pins across the globe surface. Dots are 3px diameter, spaced 8px apart, curving with the sphere's surface — must FEEL like it's wrapping around the sphere, not floating above it.
- **Avatar pins**: tiny circular profile pictures (28–34px), each with a 2px white ring and a small white teardrop pin underneath. Two pins always — one for each partner.
- **Distance / day sticker**: small white pill sticker on the globe surface (e.g., "Day 4" or "12040km"). Pill is `#FFFFFF` fill, 12px text in navy, 16px horizontal padding, 10px vertical, soft shadow.
- **Tiny continent label on cover**: a hand-drawn outline of a continent (sketch-style stroke `#FFFFFF` 70%, 1.5px) sometimes hovers off the globe as a decorative flourish.

## Floating chat / sticker bubbles

These are sprinkled around the globe and phone like a confetti of intimacy. Each is its own micro-component:

### Lavender chat bubble ("I love you" / "Miss you")
- Shape: rounded squircle, corner radius 18px, with a small triangular chat tail at the bottom-left or bottom-right
- Fill: `#B49BE6`
- Text: `#FFFFFF`, Pretendard 14–16px, weight 500
- Shadow: `#7E66B5` at 22% opacity, 16px blur, y-offset 6px
- Padding: 12px horizontal, 8px vertical
- Position on "Connect With Your Lover": two stacked bubbles upper-right of globe, slight 4deg rotation alternating directions

### Heart counter card ("214 days" / "Happy forever")
- Shape: rounded rectangle card, corner radius 20px
- Fill: `#FFFFFF`
- Inside: large soft-pink heart `#FFD7E1` with the count number `214` overlaid in navy bold serif (Tiempos or Newsreader Bold), 48px
- Subtitle below: "days" in tiny grey caps, 10px
- Shadow: `#1B2240` 8% opacity, 24px blur

### Neon pink heart (Miss You slide)
- Hot pink filled heart `#FF6FA3`, ~140px tall
- Neon glow: `#FF6FA3` at 50% opacity, 28px blur, expanding 12px outside heart silhouette
- Small "x214" or "x280" counter rendered above heart in chunky outlined display serif, navy stroke 2px, no fill (or off-white fill)
- Sits inside the phone screen, centered

### Yellow smiley / yellow heart sticker
- Buttercup `#FFE27A` heart or chat sticker, ~36px
- 2-tone shading (lighter top-left)
- Used as accent confetti — never more than 1 per slide

### "Choose" or CTA mini-pill
- White pill button with navy text, drop shadow — sits on the globe near the avatar pin on cover slide
- Border radius 999px, padding 8px 16px

Every sticker/bubble has a slight rotation (between -8deg and +8deg) — never perfectly upright. They feel hand-stuck onto the canvas.

## Pet / mascot illustrations

The Raising Pets Together slide centerpiece. Style rules:

- **Aesthetic**: flat-color kawaii, NO outlines (or very thin warm-grey `#C9B89A` outlines at 1px max). Think Line Friends / Sanrio energy but slightly more grounded.
- **Proportions**: heads are 50–55% of total body height. Bodies are squat plush-toy ovals. Legs are stubby. The whole pet should look squeezeable.
- **Faces**:
  - Eyes: solid navy `#1B2240` ovals with one tiny `#FFFFFF` catchlight dot in upper-right
  - Mouth: tiny "ω" curve or open smile with pink tongue dot
  - Cheeks: small rosy ovals `#F8B6C5` at 60% opacity, 8px wide
- **Pet roster on the slide (left-to-right)**:
  1. Husky pup — grey `#C9D2DA` with white face mask and chest, pointed ears
  2. Cream cat sitting upright — `#F4D9B0`, tiny triangle ears, curled tail visible from side
  3. Cinnamon/orange fluffy pup (Shiba-style) — `#E8A86A` with cream muzzle, curled tail
  4. Baby panda — white body with black ear, eye-patch, leg, and arm patches
  5. Optional 5th: small chick or bunny in pastel
- **Shadow under each pet**: soft ellipse `#1B2240` 12% opacity, 12px blur, 80% of pet's footprint width
- **Arrangement**: pets sit in a gentle staggered row across the lower 40% of the phone screen, each one slightly different height (varying ground line by 4–8px to feel playful, not lined-up)
- **No anthropomorphism**: pets don't hold phones or wear hats — they just exist and are cute

## Map treatment (location sharing slide)

The "Real Time Location Sharing" slide phone screen shows a soft minimal map:
- **Base color**: cream `#FBF6E8`
- **Roads**: thin white `#FFFFFF` strokes, 2–3px, no labels
- **Parks / greenery**: soft sage blobs `#D5E8D0`, irregular organic shapes
- **Water**: same pale blue as globe ocean `#BAD4F2`, irregular shape
- **No street labels, no POI icons, no compass** — the map is mood, not utility
- **Avatar pin**: large teardrop pin in white with navy avatar circle (40px) inside, dropping a soft shadow on the map. Pin sits roughly center of map.
- **Address card at bottom of phone screen**: white rounded card with "Jack" name in bold, an address line in grey, tiny phone/message icons on the right. Card has 24px corner radius and soft shadow.

The Raising Pets slide uses a different map style:
- **Base**: butter yellow `#FFF8D4`
- **Greenery patches**: same sage `#D5E8D0` in larger irregular shapes
- **Tiny path**: dotted brown `#A88962` 2px line meandering between pet positions
- **A mini house sticker**: small house illustration in upper-right corner

## Decorative accents

- Tiny 4-point stars `✦` in white at 50% opacity (`#FFFFFF`), 8–14px, scattered around globes and phones — 4–7 per slide
- Small sparkle clusters near chat bubbles (3 dots in a triangle)
- Dotted travel lines in white between avatar pins
- Cotton-puff clouds (oblong soft shapes) drifting at top of sky slides
- Small heart icons `♥` in lilac or pink as bullet points
- Faint outlined continent silhouettes floating off-globe (sketch-style stroke, 70% white)
- Soft pastel grid floor: never literal, but pets and stickers feel like they sit on an implied soft ground

NEVER use sharp geometric shapes (triangles, hard polygons), thin outlines on text, or neon gradients beyond the single hot-pink heart glow.

## Copy tone

Tone is soft, romantic, second-person, simple. Every word feels like a whisper or a sticky-note. Avoid corporate verbs ("optimize", "leverage"). Prefer warm verbs ("share", "connect", "miss", "raise", "send").

Headline examples from this mosaic:
- "10M+ Couples Use The *exclusive* app for couples."
- "Connect With *Your* Lover"
- "*Real* Time Location Sharing"
- "Raising Pets *Together*"
- "Send A Sw *'Miss You'*"

Subtitle examples (when used):
- "Always within a tap, wherever you are."
- "Track each other's location in real time."
- "Adopt and raise a pet together."

Heart emojis allowed sparingly (max 1 per slide). Lowercase ok inside speech bubbles ("i love you", "miss you").

## Per-slide breakdown

### Slide 1 — Cover ("10M+ Couples Use The exclusive app for couples.")
- **Background**: full 3-stop sky gradient (`#DDEBFA` → `#F5E0F0` → `#FCEFD6`). 3 cotton-puff clouds drifting in upper third.
- **Headline**: top-aligned, navy `#1B2240`. Reads "The" → "*exclusive*" (italic lilac serif) → "app for couples." across 3 lines. Above the headline, a tiny medallion: ribbon laurels flanking "10M+" text with 5 mini stars and "Couples Use" microcaps below. The medallion is rendered in lilac line art `#B49BE6`.
- **Hero element**: the 3D globe occupies the lower 60% of the slide, centered. Globe is ~52% of slide width. Two avatar pins visible — one on a top-left continent, one on a lower-right continent. A "Day 4" white sticker sits near the top-left pin. White dotted journey line curves between them. Faint white outlined continent silhouette floats just off the globe's upper-left as decoration.
- **Floating elements**: 5–6 tiny `✦` stars scattered around globe. No chat bubbles on cover.
- **Notable effects**: globe ground shadow below; sky has 2% noise; emphasis word "exclusive" sits at a slight optical-center bump (1px upward) to feel handwritten.

### Slide 2 — "Connect With Your Lover"
- **Background**: same sky gradient. 2 clouds upper area.
- **Headline**: top center, 2 lines. "Connect With" line 1, "*Your* Lover" line 2 (with "Your" italic lilac serif). Navy `#1B2240`.
- **Hero element**: 3D globe slightly lower-left, ~48% of slide width, ground shadow. Avatar pins on two continents, dotted journey line. A "12040km" white pill sticker sits on the globe surface near the journey line midpoint.
- **Floating elements** (with approximate positions, slide measured as 100x100):
  - Lavender chat bubble "i love you" — upper-right of globe, position (68, 38), rotation +6deg
  - Lavender chat bubble "Miss you" — directly below previous, position (72, 48), rotation -4deg
  - Yellow heart sticker — small, position (24, 22), rotation -12deg
  - White "Choose" mini-pill — sitting on globe near right avatar pin, position (62, 70)
  - 4–5 `✦` stars scattered
- **Notable effects**: chat bubbles cast lavender shadows; globe overlaps slightly with the right edge of the chat bubble stack.

### Slide 3 — "Real Time Location Sharing"
- **Background**: same sky gradient, slightly desaturated since the phone screen takes most attention. 1 cloud.
- **Headline**: top center, 2 lines. "*Real* Time" line 1 (with "Real" italic lilac), "Location Sharing" line 2.
- **Hero element**: phone, ~62% of slide width, centered. Phone screen displays the soft minimal map (cream base, sage parks, white roads, baby-blue water blob). One avatar pin (teardrop with circular avatar) centered on the map.
- **Phone screen UI**:
  - Top status bar minimal
  - Tiny avatar circles of "friend" pings in upper map area
  - Bottom card: white rounded card with "Jack" name in bold, address subtext in grey, two tiny circular action icons (call, message) on the right
  - Below card a slim status pill: "100m" or similar walking indicator with a small avatar
- **Floating elements**: minimal — maybe 2 stars off to the sides. No chat bubbles on this slide.
- **Notable effects**: phone has soft drop shadow; map inside phone has its own subtle inner shadow at the screen edges.

### Slide 4 — "Raising Pets Together"
- **Background**: sky gradient transitions to slightly more peach (`#F5E0F0` → `#FCEFD6` dominant). 1 cloud.
- **Headline**: top center, 2 lines. "Raising Pets" line 1, "*Together*" line 2 (italic lilac serif).
- **Hero element**: phone, ~62% of slide width, centered. Phone screen interior is a butter-yellow `#FFF8D4` map with sage green patches and a small house illustration in the upper-right.
- **Phone screen contents**:
  - Top status pill showing a tiny pet name card ("Doghana" or similar) with `+` icon
  - Lower 40% of screen: row of 4 kawaii pets (husky, cream cat, cinnamon pup, panda) sitting on the implied yellow ground, each with a small shadow ellipse
  - Bottom action bar: a "Honey" or partner name pill with a heart and emoji reactions
- **Floating elements**: tiny strawberry or fruit stickers scattered, 2 stars. No chat bubbles.
- **Notable effects**: pets have no outlines; small dotted brown path between pets; sky cloud overlaps just behind the top of the phone.

### Slide 5 — "Send A Sw 'Miss You'"
- **Background**: warm peach-pink rotation of gradient (`#FBE3D7` → `#F7C9D6` → `#F3B6CC`). 1–2 clouds.
- **Headline**: top right of phone, 2 lines. "Send A Sw" line 1, "*'Miss You'*" line 2 (quoted phrase in italic lilac serif). Slight left-margin offset because phone hugs the left edge.
- **Hero element**: phone, ~60% of slide width, positioned slightly left of center. Phone screen interior:
  - Top: "x214" or "x280" chunky outlined display-serif counter in navy
  - "Live Activities" tiny header row
  - Center: massive hot-pink `#FF6FA3` heart with neon glow, "214" rendered in white display serif inside the heart
  - Bottom: a row of emoji reactions (cute faces, hearts) on small white circle backgrounds
  - "Happy forever ✦" tiny line above the heart
- **Floating elements**: 3–4 tiny stars; 1 small white chat bubble peeking from upper-left with "Miss you" text
- **Notable effects**: hot-pink heart casts the only "neon" glow in the entire mosaic — this is the one allowed exception to the soft-pastel rule, and it should still feel cushioned, not aggressive.

## How to apply this style

1. Start with the 3-stop sky gradient (`#DDEBFA` → `#F5E0F0` → `#FCEFD6`) on a 1242×2688 canvas. Add 1.5% monochrome noise overlay at `mix-blend-mode: overlay`.
2. Drop 2–3 cotton-puff cloud SVGs at 10% opacity in the upper third with a 6px gaussian blur.
3. Place the phone or globe as the hero element. Phone gets a 14% navy drop shadow at 40px blur, 16px y-offset. Globe gets a 10% ellipse ground shadow.
4. Write the headline in Pretendard Semibold navy `#1B2240`. Identify the ONE emotionally-loaded word and wrap it in a span: italic, serif (Newsreader Italic), lilac `#B49BE6`, 1.05× size.
5. Add floating sticker elements: lavender chat bubbles with tails, white heart counters, yellow accents, white "Day N" or distance pills. Rotate each between -8deg and +8deg. Cast appropriate colored shadows.
6. Sprinkle 4–7 `✦` stars in white at 50% opacity around the hero.
7. If pets are involved: render flat-color kawaii pets with no outlines, dot eyes with catchlights, rosy cheeks, soft ellipse shadows. Stagger them on the ground line.
8. Keep negative space generous — the dreaminess comes from breathing room.

## What this style is NOT

- Do NOT use harsh saturated reds, electric blues, or any color outside the pastel palette (the single hot-pink heart on slide 5 is the only neon exception).
- Do NOT use bold ALL-CAPS headlines — title case only, mixed weight.
- Do NOT use realistic photography of people, pets, or places — illustrations and 3D-rendered globes only.
- Do NOT omit the italic serif emphasis word — every headline must have exactly one.
- Do NOT use heavy outlines on pets or stickers — flat color, optional thin warm-grey only.
- Do NOT use rigid grid layouts — stickers and bubbles must rotate slightly and feel hand-placed.
- Do NOT use corporate sans-serifs like Helvetica or Arial — Pretendard / Apple SD Gothic / Inter only.
- Do NOT use crisp drop shadows — all shadows are large radius, low opacity, soft.
- Do NOT use more than ONE chat bubble cluster per slide — they should feel like punctuation, not decoration.
- Do NOT render the globe as a realistic Earth (no satellite imagery, no political borders) — it must be the stylized mint-on-blue pastel sphere.
- Do NOT use emojis as primary visual elements — use illustrated stickers; emojis only inside chat bubbles or reaction rows.
- Do NOT use serif fonts for body or subtitles — serif italic is reserved exclusively for the emphasis word.
- Do NOT center every line — vary the headline placement (top center, top left) slide-to-slide.
- Do NOT let clouds become a focal element — they are atmosphere at <12% opacity.
- Do NOT make pets look realistic or angular — round, plush, squeezeable proportions only.
