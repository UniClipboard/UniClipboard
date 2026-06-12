---
name: moody-curated-dating
description: Cinematic, dimly-lit lifestyle photography overlaid with white serif headlines and italic emphasis. Curated, exclusive, member's-club feel. Inspired by Mate.
inspiration: Mate, Raya, Pair Eyewear premium ads, The Inner Circle, Aman resorts campaign aesthetic
feel: intimate, candle-lit, "you've been hand-picked"
---

# Moody Curated Dating

> **READ FIRST:** [`./_QUALITY_BAR.md`](./_QUALITY_BAR.md) — universal quality rules apply to this style.

## Hard quality rules (this style)

- **Headline font size MUST be large.** On a 1320×2868 canvas, the cream serif headline must render at **≥ 132pt (≈ 176 px) per line** — minimum cap-height ~115 px. Previous renders at 78–82pt read as "small text floating in a dark void" and is a hard fail for this style. Use **140–160 px font-size** as your default and only drop if the line genuinely cannot fit at 88% canvas width. Line-height ≤ 1.08. The subhead/italic underline can be 40–50 px.
- **Phone size on phone-bearing slides**: phone height = **78–88%** of canvas height. Cover/hero slides without a phone keep the same headline-size rules.
- **Photography or photo-grade gradient is mandatory**. A simple 2-stop radial gradient on black is NOT enough. Build the fake-photo with at least: (a) a warm tungsten radial off-center at `#C99566 → #8A5A33 → #1F1C1A → #0A0A0B`, (b) a candle-bokeh blob (50px blur, 12% opacity, warm amber) in the upper third, (c) a teal-shifted shadow blob (40px blur, 8% opacity, `#1A2026`) on one side, (d) a radial vignette overlay `radial-gradient(ellipse at center, transparent 35%, rgba(0,0,0,0.62) 100%)`, (e) a bottom linear `linear-gradient(180deg, transparent 0%, rgba(0,0,0,0.85) 100%)` covering the bottom 35–50%, (f) a 5% film-grain SVG overlay with `mix-blend-mode: overlay`. Skip any one and the slide reads as a plain dark gradient.
- **Headline serif weight = 400 (regular)**, never bold or black. The contrast comes from italic, not weight. If your headline reads as "thick" you have used the wrong weight.
- **Italic phrase MUST exist on every slide** that has a headline. Same color, same weight, italic only. Without it, the headline is generic.
- **macron diacritic over the wordmark `a`**: a single horizontal cream `#F4EBDD` bar, length ~14% of cap-height, positioned one stem-width above the bowl of the `a`. Required on every wordmark instance. Omitting it breaks the lockup.
- **Phone tilt = 0°**. Tilted phones are a fail in this style.
- **No saturated accent colors anywhere**. Gold/candle-amber `#C99566`/`#E8B97A` is the only accent. Adding red/green/blue chips kills the mood.
- **Photo never crops through eyes or jawline** when actual photos are used.

## Vibe summary
This style sells exclusivity through atmosphere, not through chrome or gradients. Every slide reads like a still from a 35mm short film about a private dinner club — warm tungsten light spilling across faces, deep teal-black shadows swallowing the edges, and a quiet serif headline floating like a whispered line of dialogue. The voice is selective, second-person, and a little romantic: "your circle," "hand-picked," "worth your time." Nothing shouts. The photography does all the heavy lifting, and the UI hides almost entirely so the brand reads as a lifestyle, not a product.

## Global palette
Use these values as Tailwind arbitrary values or CSS custom properties.

| Token | Hex | Use |
|---|---|---|
| `--ink-0` | `#0A0A0B` | Deepest background, crushed-black shadows |
| `--ink-1` | `#0E0E10` | Phone screen background |
| `--ink-2` | `#16151A` | Phone card surfaces (solid fallback) |
| `--ink-3` | `#1F1C1A` | Warm-shifted shadow on photo darkest zones |
| `--vignette` | `rgba(0,0,0,0.62)` | Corner darkening at 40% from edges |
| `--bottom-fade-top` | `rgba(0,0,0,0)` | Top of bottom gradient (35% from bottom) |
| `--bottom-fade-bot` | `rgba(0,0,0,0.78)` | Bottom of bottom gradient |
| `--tungsten` | `#C99566` | Warm key-light highlight on skin |
| `--tungsten-deep` | `#8A5A33` | Warm mid-tones, lamp glow |
| `--candle` | `#E8B97A` | Specular candle/bulb highlights |
| `--teal-shadow` | `#1A2026` | Cool shadow side of faces (orange-teal grade) |
| `--cream` | `#F4EBDD` | Headline color, wordmark |
| `--cream-dim` | `rgba(244,235,221,0.72)` | Subhead opacity |
| `--cream-mute` | `#B8AFA2` | Phone body copy |
| `--gold-hairline` | `rgba(201,149,102,0.55)` | 1px ring around avatars/dividers |
| `--card-fill` | `rgba(255,255,255,0.04)` | Glass cards inside phone |
| `--card-border` | `rgba(255,255,255,0.08)` | 1px card border |
| `--notif-fill` | `rgba(28,26,24,0.78)` | Floating notification background |
| `--notif-border` | `rgba(255,255,255,0.06)` | Notification hairline |
| `--grain-opacity` | `0.05` | Film noise layer alpha |

Color philosophy: zero pure white, zero pure saturated colors. Even the brightest highlight is the warm cream, never `#FFFFFF`. Even "black" is a 1-2% warm-shifted dark, never `#000`.

## Photography rules (signature)

These are non-negotiable. The photography IS the brand.

- **Color grade:** classic teal-orange. Highlights pushed to warm amber `#C99566` / `#E8B97A`. Shadows pushed to cool teal `#1A2026`. Mids slightly desaturated. Hue shift roughly +8 on highlights warm, -12 on shadows cool.
- **Saturation:** sit it at ~70% of source. Reds should never glow; skin should look candle-lit, not Instagram-filtered.
- **Contrast:** high, with crushed blacks. Black point lifted to about RGB 8-12. Highlights rolled off so they don't clip — keep them around 235, never 255.
- **Vignette:** heavy radial darkening. Center at ~95% luminance, corners at ~55%. Use a radial-gradient overlay: `radial-gradient(ellipse at center, transparent 35%, rgba(0,0,0,0.62) 100%)`.
- **Subjects:** candid moments only — dinner parties, two friends embracing, head thrown back laughing, hand on glass of wine, eye-contact portrait at a bar. NEVER staged headshots, NEVER stock smiles, NEVER white-background product shots.
- **Skin tone preservation:** even with the warm grade, do not let skin go orange. Pull yellow out of skin slightly so warmth reads as light, not jaundice. Maintain ethnic skin-tone integrity — the grade should flatter every tone, not flatten differences.
- **Lighting:** warm key from side or above. Think practical lights — Edison bulbs, taper candles, a single window. Never on-camera flash, never softbox-flat. A visible falloff into shadow on one side of the face is mandatory.
- **Film grain overlay:** 4–6% monochrome noise on top of the entire photo. Use a tiled SVG noise or a `filter: url(#grain)` turbulence node. Grain stays consistent across all 5 slides for set cohesion.
- **Bottom gradient overlay:** the lower 35% of every photo gets a top-down linear-gradient `linear-gradient(180deg, rgba(0,0,0,0) 0%, rgba(0,0,0,0.78) 100%)`. This is what makes the white headlines legible without a card.
- **Top gradient (light):** the top 12% gets a subtle `linear-gradient(180deg, rgba(0,0,0,0.35), rgba(0,0,0,0))` so the wordmark sits cleanly.
- **No HDR clipping, no halation, no chromatic aberration.** This is cinema-grade, not filter-app.

## Typography

- **Headline face:** thin to regular serif with high contrast strokes. Use one of: Tiempos Headline, GT Sectra, Canela, or Recoleta as a free fallback. Weight 400 (regular) for body of the headline, italic 400 for the emphasis phrase. Optical size: display.
  - Headline size at 1290×2796 canvas: **78pt** (≈104px) for the dominant title, **62pt** (≈82px) for shorter titles on slides with phones.
  - Line-height: 1.08. Tracking: −0.5%. Hanging punctuation off.
  - Color: `--cream` `#F4EBDD`. No drop shadow. Optional 0–1px text-shadow `rgba(0,0,0,0.35)` only if grain breaks legibility.
- **Subhead:** same serif family, **34pt** (≈45px), regular roman, color `--cream-dim` (cream at 72% opacity). One short line, no period required.
- **Phone UI body:** clean grotesque sans — Inter, Söhne, or SF Pro Display. Sizes inside phones: 13pt header, 11pt body, 9pt caption. Weight 500 for titles, 400 for body.
- **Wordmark — `mate`:** lowercase serif, same headline face, weight 400, size **44pt** (≈58px) in the top-left of slide 1 and centered below the phone on slide 2. The wordmark has a **diacritic-like macron or hairline stroke above the `a`** — a single horizontal short bar (~14% of cap-height long) sitting one stem-width above the bowl of the `a`, drawn in `--cream`. It evokes a long-vowel diacritic (mā́te) and is part of the lockup. Always render the mark; never strip it.
- **Tracking, leading, optical size:**
  - Display headlines: `letter-spacing: -0.005em`, `line-height: 1.08`.
  - Subheads: `letter-spacing: 0`, `line-height: 1.3`.
  - Phone UI: `letter-spacing: -0.01em`, `line-height: 1.35`.

## Headline emphasis

Every headline contains ONE italicized phrase, never two. The italic phrase always carries the emotional payload of the line. Color stays identical to the rest of the headline (no color shift, no weight shift — italic only). Examples taken from the deck:

- "Every Thursday, with people at your level. *Upgrade your circle.*" (italic on the imperative)
- "We make a filter so *every person you meet is worth your time.*" (italic on the promise)
- "Every Thursday, We sit you at a table of 6 *hand-picked members.*" (italic on the differentiator)
- "Every day, *Limited curated profiles. Quality is key.*" (italic on the value prop)
- "*Complete profil[es]* — deep conversations." (italic on the product noun)

Rule: italicize the part that, if removed, would leave the line generic.

## Phone / device frame treatment

- **Bezel:** matte black, very thin — 6–8px equivalent at canvas scale. No silver edge, no notch chrome reflection.
- **Body:** flat black `#0A0A0B`, no gloss. A subtle 1px inner stroke at `rgba(255,255,255,0.04)` to separate bezel from screen.
- **Screen background:** `--ink-1` `#0E0E10`. Never pure black, never a gradient.
- **Inner glow:** an inset shadow on the screen `inset 0 0 60px rgba(201,149,102,0.06)` — a barely-there warm halo from the photo bleeding around the phone.
- **Drop shadow:** soft and warm-tinted. `0 40px 80px rgba(0,0,0,0.55), 0 8px 16px rgba(60,30,10,0.25)`.
- **Tilt:** essentially none. Phones sit upright, perpendicular to the canvas. Slight ±1° rotation is acceptable for life; never more.
- **Status bar:** rendered, monochrome white at 80% opacity, time `9:41`, full signal, full battery. Never colored carrier text.
- **Home indicator:** 5px tall white pill at 35% opacity, 35% of screen width, centered, 8px from bottom.

## Background photo placement & cropping

- **Full-bleed:** photo covers the entire 1290×2796 slide, edge-to-edge. Zero margin, no rounded corners on the photo itself (the App Store does that).
- **Subject placement:** off-center. Subjects sit on a vertical third — usually left third or right third — leaving the opposite negative space for typography. On full-screen-photo slides (no phone), subjects can also occupy upper third with text in the lower third.
- **Rule of thirds:** eyes of the primary subject sit on the upper third-line. Hands, glasses, and gestures live on the lower third-line.
- **Face safety:** never crop through eyes, never crop at the jawline. Hairline or shoulders are acceptable crop points.
- **Photo extends below the headline gradient.** The image itself never stops where the text starts — it fades behind via the gradient overlay.
- **Aspect handling:** if a source image is too tall or too wide, scale-to-fill (`object-fit: cover`) and bias the focal point toward the off-center subject.

## Floating UI elements (notifications/stickers)

- **Notification card (slide 2 — "Welcome to the circle"):**
  - Shape: rounded rectangle, `border-radius: 22px` (translates to ~28px at canvas scale).
  - Size: ~78% of phone screen width, 96px tall at canvas scale.
  - Padding: 16px horizontal, 12px vertical.
  - Fill: `--notif-fill` `rgba(28,26,24,0.78)` with `backdrop-filter: blur(28px) saturate(120%)`.
  - Border: 1px `--notif-border` `rgba(255,255,255,0.06)`, plus an inner 1px `rgba(255,255,255,0.04)` highlight at the top edge for glass realism.
  - Shadow: `0 20px 40px rgba(0,0,0,0.45)`.
  - Content: small square app icon (22px, gold M monogram on dark) on left; bold title "Welcome to the circle." in 13pt Inter Semibold cream; body "Membership approved, you're a perfect fit. Only 1 in 5 applicants passes the filter." in 11pt Inter Regular at `--cream-mute`.
  - Position: bursts out of the phone — its right edge extends ~8% beyond the phone bezel, sitting roughly vertically centered on the screen. Gives the impression the notification is "lifting off" the device.
- **Profile sticker bubbles (slide 3):** small circular avatars overlapping the phone:
  - Diameter: ~84px at canvas scale.
  - Border: 2px solid `--cream` with an outer 1px `--gold-hairline` ring (two-tone ring effect).
  - Drop shadow: `0 8px 20px rgba(0,0,0,0.55)`.
  - Photo inside is the same warm-graded portraiture.
  - Three bubbles per cluster, slightly staggered vertically, the topmost one bleeding 30% off the right bezel.
- **Wordmark sticker:** the `mate` wordmark with macron sits cleanly on photo (slide 1, top-left) or below phone (slide 2, centered). Never inside the phone screen.

## Dark phone UI inside screenshots

- **App background:** solid `--ink-1` `#0E0E10`.
- **Cards:** fill `--card-fill` `rgba(255,255,255,0.04)`, border `1px solid --card-border` `rgba(255,255,255,0.08)`, `border-radius: 16px`, internal padding 16px.
- **Profile photos in lists:** square or rounded-square with `border-radius: 12px`, wrapped in a `1px solid --gold-hairline` ring with 2px inset gap (use double box-shadow trick: `0 0 0 2px var(--ink-1), 0 0 0 3px var(--gold-hairline)`).
- **Profile grid (slide 4):** 2-column grid of portrait cards, gap 8px, each card aspect 3:4, name overlaid bottom-left in 11pt Inter Medium cream with a small `linear-gradient(transparent, rgba(0,0,0,0.7))` foot. Tags/age live in 9pt under the name.
- **Schedule / time pills (slide 3 — "Your table for The Thursday Ritual is ready"):**
  - Pill: `border-radius: 9999px`, padding 6px 12px, fill `rgba(201,149,102,0.10)`, border `1px solid rgba(201,149,102,0.30)`, text in `--candle` `#E8B97A` at 11pt Medium.
  - Icon (calendar / pin): 12px line-icon in `--candle`, stroke 1.5px.
- **Buttons:** ghost style only — `1px solid rgba(255,255,255,0.18)`, text cream, padding 12px 20px, `border-radius: 12px`. No filled CTAs inside the screen.
- **List item text hierarchy:** name 13pt SemiBold cream; meta 11pt Regular `--cream-mute`; tertiary 9pt at 50% opacity.
- **Tab bar (slide 4):** 5 monoline icons, stroke 1.5px, active icon cream, inactive `rgba(255,255,255,0.35)`. No labels.
- **Profile screen (slide 5):** photo top with red/cream tag pills (e.g., "Tapas, cocktails and good conversations") overlaid in `rgba(201,149,102,0.18)` fill with `--candle` text, plus a horizontal interest bar / progress strip in muted gold below.

## Decorative accents

- Wordmark lockup with macron (always required where wordmark appears).
- Optional gold hairline divider — `1px` `--gold-hairline`, width ~12% of canvas, horizontally centered, used between wordmark and subhead.
- Soft warm noise grain layered globally (~5% opacity).
- A single small candle-glow bokeh blur in upper third on dinner-table slides — natural to the photo, never added as graphic element.
- **No icons floating freely on the background. No badges. No "App Store Rating 4.9". No arrows. No emoji. No confetti. No gradient blobs.** The photography carries the deck.

## Copy tone

- **Voice:** intimate, selective, occasionally second-person ("your circle", "your level", "your time"). Confident understatement.
- **Vocabulary palette:** curated, hand-picked, your circle, worth your time, ritual, filter, table of 6, members, quality, level, profile, deep conversation, Thursday.
- **Avoid:** "swipe", "match", "find love", "soulmate", any superlative ("best", "top", "#1"), exclamation marks, emojis, all-caps words, abbreviations.
- **Sentence case only.** Periods end every line. No question marks in headlines.
- **Numbers spelled out** below ten ("a table of six" preferred), digits acceptable for time/dates ("Thursday").
- **Actual lines from the deck:**
  - "Every Thursday, with people at your level. Upgrade your circle."
  - "Congrats, Andy"
  - "We make a filter so *every person you meet is worth your time.*"
  - "Your table for The Thursday Ritual is ready"
  - "Every Thursday, We sit you at a table of 6 *hand-picked members.* 3 boys, 3 girls"
  - "Every day, *Limited curated profiles. Quality is key.*"
  - "*Complete profil[es]* — deep conversations."

## Per-slide breakdown (mandatory)

### Slide 1 — "Upgrade your circle" (group dinner hero)
- **Background photo:** warm dinner-party scene, two-row composition. Top half: a candle-lit table with several people, glasses raised mid-toast, deep crimson velvet/curtain behind them. Bottom half: two women in foreground embracing or leaning into each other, laughing. Color cast: heavy tungsten amber on faces and glasses, deep teal-black behind the figures. Crimson reds in clothing and curtain are desaturated to brick.
- **Vignette / gradient overlay:** radial vignette darkening the four corners by ~40%. A central vertical band stays slightly brighter so the wordmark at top and headline at middle both read.
- **Headline:** "Every Thursday, with people at your level." in cream serif, three-line stack centered horizontally, sitting at vertical middle (52–60% from top). Below it, a smaller line "Upgrade your circle." in subhead size at 72% opacity cream — this is where the italic phrase lives if you choose to italicize the subhead instead of inside the headline.
- **Wordmark:** `mate` (with macron) top-left, 44pt, 56px from top edge, 56px from left.
- **Phone:** NONE. Full-bleed lifestyle photo only.
- **Floating elements:** none.
- **Notable effects:** strongest grain on this slide because the dim midtones reveal it most. Slight warm halation around the brightest candle highlight is acceptable.

### Slide 2 — "Congrats, Andy" (acceptance moment)
- **Background photo:** medium close-up of a young man with longish dark hair under warm bar light, half-smile, looking slightly off-camera. Background blown out to a soft tungsten haze (deep golden bokeh). Strong key from upper-right, shadow falls across left cheek.
- **Vignette / gradient overlay:** standard radial vignette; bottom gradient takes over the lower 30% to cradle the wordmark.
- **Headline:** "Congrats, Andy" in cream serif, two-line stack with a comma break, centered horizontally, sitting at the top third (around 18–24% from top). Size 78pt. No italic (this slide uses the floating notification card as the rhetorical beat instead).
- **Phone:** NONE — the entire screen IS the "phone view" in a sense, but no device frame is rendered. The notification floats directly on the photo as if Andy just got the push.
- **Floating elements:** the "Welcome to the circle." notification described above, horizontally centered, sitting at ~56% from top. App icon on the left of the card is a small dark square with a gold `m` monogram.
- **Wordmark:** `mate` (with macron) centered below the notification card, ~22% from bottom, 44pt cream.
- **Notable effects:** the notification card's backdrop-blur should genuinely blur the photo behind it — this is the moment of contact between brand and user.

### Slide 3 — "Your table for The Thursday Ritual is ready" (booking confirmation phone)
- **Background photo:** dim dinner table or bar interior, very dark, mostly out-of-focus warm bokeh. Subject (likely a hand pouring wine or a candle) sits low-left or low-right. Used mainly as atmosphere — the phone is the hero.
- **Vignette / gradient overlay:** heavier than other slides — the photo is functionally a moody backdrop. Bottom gradient extends up to 50% to anchor the headline.
- **Phone:** upright, centered horizontally, occupies vertical 18–78% of canvas. Screen content:
  - Top: small cream serif title "Your table for The Thursday Ritual is ready" centered, 13pt.
  - Two amber-tinted pills stacked: calendar pill with date "Tuesday, December 20", location pill "Carrer Casp 24, Barcelona" (or similar). Both use the candle-amber tint described above.
  - Center: three overlapping circular avatars (the cluster described in floating elements) — but these specific ones live inside the phone screen on a dark card, with the cream/gold ring treatment, suggesting the table of 6 starting to fill.
  - Bottom: ghost button "Let's go" or similar in cream ghost-button style.
  - Wordmark `mate` with macron sits at the bottom of the screen, just above the home indicator, in cream 18pt.
- **Floating elements:** profile sticker bubble(s) may bleed slightly off the phone's right bezel as noted.
- **Headline (outside phone):** "Every Thursday, We sit you at a table of 6 *hand-picked members.*" in cream serif, 3-line, centered horizontally below the phone (~85% from top). Italic on "hand-picked members." Subhead beneath: "3 boys, 3 girls" in 28pt cream at 60% opacity.
- **Notable effects:** the phone's inset warm glow is most visible here because the surrounding photo is darkest.

### Slide 4 — "Quality is key" (curated profile grid phone)
- **Background photo:** very dark — a single subject (woman in profile, half-lit, soft smile) on the right side, mostly silhouetted against a tungsten-glow background. The photo's job here is to frame the phone.
- **Vignette / gradient overlay:** strong radial vignette; bottom gradient again pulls the headline forward.
- **Phone:** upright, slightly left-of-center to balance the right-side photo subject. Screen content:
  - Dark `--ink-1` background.
  - Top status bar.
  - 2×N grid of portrait cards (4 visible: a curly-haired woman, a man in blue tracker, a man with glasses labeled "Julian", a woman with red hair). Each card has the gold-hairline ring described, with name + age overlay bottom-left.
  - Bottom tab bar with 5 monoline icons as specified.
- **Floating elements:** the small "Tracker 11" or "Julian" name labels — these are inside the cards, not floating.
- **Headline (outside phone):** "Every day, *Limited curated profiles. Quality is key.*" centered below phone in cream serif, italic on "Limited curated profiles. Quality is key." Position ~84% from top.
- **Notable effects:** the grid cards' gold rings catch warm light from the photo behind — keep that gold ring opacity high enough (50–60%) so it reads as part of the brand language.

### Slide 5 — "Complete profiles, deep conversations" (single profile detail phone)
- **Background photo:** waist-up portrait of a woman, looking directly at camera, soft smile, warm candle-amber light from frame-left. Brown/charcoal background, slightly out of focus. Most photographically forward slide of the deck.
- **Vignette / gradient overlay:** lighter vignette than slides 3 and 4 because the portrait itself is the hero alongside the phone.
- **Phone:** upright, positioned right-of-center or centered. Screen content:
  - Profile photo at top filling ~55% of screen height.
  - Overlaid name "Lucía" in cream serif 22pt, bottom-left of the photo, with a 4px round-cap underline in `--candle` directly beneath.
  - Two interest tags as candle-amber pills: e.g., "Tapas, cocktails and good conversations" — pill wraps to two lines, full-width minus 16px margins, gold-tinted.
  - Horizontal "interest bar" — a 4px tall progress-style strip in muted gold (`--tungsten-deep` fill, `--candle` highlight at ~40% width), no label, just a visual.
  - Below it, secondary metadata in `--cream-mute` 11pt: language, neighborhood, profession in one line separated by `·`.
- **Headline (outside phone):** "*Complete profil[es]* — deep conversations." or similar, italic on "Complete profiles", positioned bottom of canvas at ~92% from top, may be partially cropped in mosaic preview as is intentional in the deck.
- **Notable effects:** subtle 1px cream underline beneath "Lucía" should be drawn as a separate element with rounded caps, not text-decoration, so it has presence.

## How to apply this style

1. **Source the photograph first.** Choose a dim, candid lifestyle shot with a single clear off-center subject and natural warm lighting. If you don't have a great photo, this style fails — don't substitute.
2. **Apply the orange-teal grade.** In CSS, layer: a `mix-blend-mode: multiply` warm-amber overlay at 8% opacity on top of the photo, plus a `mix-blend-mode: screen` cool-teal at 5% on shadows (via mask). Easier path: pre-grade in source and serve as JPG.
3. **Apply radial vignette** as a positioned overlay div with `radial-gradient(ellipse at center, transparent 35%, rgba(0,0,0,0.62) 100%)`.
4. **Apply the bottom linear gradient** as another absolutely-positioned div covering the lower 35–50% of the canvas with `linear-gradient(180deg, rgba(0,0,0,0) 0%, rgba(0,0,0,0.78) 100%)`.
5. **Apply the global film grain** as a fixed `pointer-events:none` overlay using a tiled SVG noise at 5% opacity.
6. **Place the serif headline** in cream, centered, with one italic phrase. Use Tiempos / GT Sectra / Canela / Recoleta.
7. **Add the wordmark** with the macron diacritic above the `a`. Same family as headline.
8. **If using a phone**, render it upright with thin black bezel, dark `#0E0E10` screen, glass cards, gold-hairline avatar rings, candle-amber pills, ghost buttons. No filled CTAs.
9. **Add at most ONE floating element** per slide — either a notification card OR a profile bubble cluster, never both on the same slide.
10. **Step back.** If anything reads loud, remove it. The style's strength is restraint.

## What this style is NOT

- Do not use bright daylight, pastel, or high-key photography. No beach, no white sheets, no flat outdoor light.
- Do not use sans-serif headlines. Ever.
- Do not use a bold or black weight serif. Headlines are regular or thin — the contrast comes from italic, not weight.
- Do not use solid color backgrounds, gradients-as-backgrounds, or blurred photo backgrounds in lieu of real photography. The photo IS the background.
- Do not use neon, electric blue, magenta, lime, or any saturated accent color. Gold/candle-amber is the only accent.
- Do not use rounded card containers around the headline. Text floats directly on the photo with help only from the bottom gradient.
- Do not use pure `#FFFFFF` text or pure `#000000` shadows. Cream and warm near-black only.
- Do not tilt phones, do not put them in clay-style 3D scenes, do not use floating shadows that look like Figma defaults.
- Do not use emojis, sticker graphics, badges, app store rating banners, arrows, or "As seen in" press logos.
- Do not stack more than 3 lines of headline text. If the line doesn't fit in 3, cut copy.
- Do not exclaim, do not ask a question, do not use all-caps. Sentence case with periods only.
- Do not include a CTA button in the screenshot graphics ("Download Now" etc). The App Store provides the CTA — your job is mood.
- Do not show filled brand-colored buttons inside the phone UI. Ghost buttons only.
- Do not show match percentages, swipe arrows, "It's a match!" graphics, hearts, flames, or any other dating-app cliché. This brand sells dinner, not Tinder.
- Do not let skin tones go orange. Warmth is in the light, not in the skin.
- Do not omit the macron over the `a` in the `mate` wordmark. It is the lockup.
