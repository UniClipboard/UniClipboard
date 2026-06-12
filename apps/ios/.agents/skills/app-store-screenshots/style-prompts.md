# Style Prompts

A library of named visual styles distilled from the best App Store screenshots in the wild. When a user names one of these styles (or pastes one of the prompts), the screenshot deck should adopt the **entire spec** in the matching file — palette, typography, layout rhythm, decorative accents, and copy tone.

Apply a style globally first; let the user override specific slides afterwards.

If a user gives a prompt that does not match a named style, fall back to the General Visual Design Principles in `SKILL.md` and pick the **closest** style here as the starting point.

---

## Deep style spec index (`./style-prompts/`)

**Before you read any deep spec, read [`./style-prompts/_QUALITY_BAR.md`](./style-prompts/_QUALITY_BAR.md).** It defines the universal sizing, illustration-polish, contrast, and auto-reject rules that apply to every style. Each deep spec adds style-specific quantitative rules on top.

**When a user names one of these styles, ALWAYS read the matching deep spec file before generating slides.**

| # | Slug | Deep spec file | Use when… |
|---|------|----------------|-----------|
| 01 | `retro-rubberhose-mascot` | [01-retro-rubberhose-mascot.md](./style-prompts/01-retro-rubberhose-mascot.md) | Cozy walking/habit app with a 1930s cartoon character. Cream + mustard, white-gloved mascot, chunky retro display. Inspired by Cancoco. |
| 02 | `moody-curated-dating` | [02-moody-curated-dating.md](./style-prompts/02-moody-curated-dating.md) | Exclusive members-only dating / dinner clubs. Dim lifestyle photography, white serif headlines with italic emphasis. Inspired by Mate. |
| 03 | `paper-sticker-skeuomorphic` | [03-paper-sticker-skeuomorphic.md](./style-prompts/03-paper-sticker-skeuomorphic.md) | Student organizer, notes, hobby apps. Cork-board bg, paper-cutout UI, marker handwriting, sticker pixel-art. Inspired by Folderly. |
| 04 | `dreamy-pastel-couples` | [04-dreamy-pastel-couples.md](./style-prompts/04-dreamy-pastel-couples.md) | Couples / long-distance / pet-companion apps. Cotton-candy sky gradient, 3D globe, kawaii pets, lilac italic serif emphasis. Inspired by Between. |
| 05 | `hand-drawn-editorial-tasks` | [05-hand-drawn-editorial-tasks.md](./style-prompts/05-hand-drawn-editorial-tasks.md) | Productivity / tasks / notes with designer taste. Navy + cream + coral slides, script accent word, tilted phones, doodle squiggles. Inspired by Superlist. |
| 06 | `glossy-3d-kbeauty-creator` | [06-glossy-3d-kbeauty-creator.md](./style-prompts/06-glossy-3d-kbeauty-creator.md) | K-beauty / creator-economy / influencer-brand collab. Deep purple gradient, glossy chrome 3D numerals, kawaii ghost mascot, yellow hashtag chips. Inspired by Nuri Lounge. |

---

## How to apply a style

When a user invokes a style by name (e.g. "use the Hand-Drawn Editorial style"):

1. Read the matching deep spec file (and `_QUALITY_BAR.md` first).
2. Set the theme palette to the listed hex values.
3. Set the font family for headline + body to the listed stack.
4. Match the listed headline emphasis behavior.
5. Apply the listed layout rhythm (which slide types and in what order).
6. Add the listed decorative accents on at least 60% of slides.
7. Rewrite the copy tone to match the listed voice rules.

If the style and a user-supplied brand color clash, keep the brand color and adjust adjacent palette entries to be analogous.

---

## Combining and overriding

- If the user names a style **and** gives custom brand colors, swap the palette colors closest in lightness to the brand colors but keep the typography and accent rules intact.
- If the user names a style **and** a layout (e.g. "style 05 but with style 04's layout rhythm"), keep the named style's typography + accents, swap only the layout rhythm.
- If two styles are named, pick the first as primary and pull at most one element (typography OR decoration) from the second.

## Adding new styles

When the user describes a new style not in this list and likes the result, propose appending a new deep spec file in `./style-prompts/` using the next sequential number, following the same field layout as the existing specs (Palette / Typography / Headline emphasis / Layout rhythm / Decorative accents / Copy tone), then add a row to the index table above.
