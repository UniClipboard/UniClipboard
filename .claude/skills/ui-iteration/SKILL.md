---
name: ui-iteration
description: "Agent Loop for pixel-perfect UI adjustments: extract a structured spec from the user's intent, apply changes one property at a time, self-verify via screenshot, and converge instead of ping-ponging. Designed for high-fidelity UI cloning and spacing/style iteration."
user-invocable: true
allowed-tools: Bash(git:*), Bash(grep:*), Bash(rg:*), Bash(find:*), Bash(cat:*), Bash(npx:*), Bash(bun:*), Bash(npm:*), Bash(xcrun:*), Bash(screenshot:*), Read, Edit, Write, AskUserQuestion, Agent
---

# ui-iteration

## Purpose

An Agent Loop for UI polish work where the user needs pixel-level adjustments and the current pattern ping-pongs between overshooting and undershooting.

**Observed anti-pattern (Session S47: 29 prompts!):**
```
User: "padding 太大"
Agent: [changes 24→16]
User: "太小了"
Agent: [changes 16→28]
User: "还是不对, 调到 24"
Agent: [changes 28→24]  ← back to where we started
User: "horizontal 也调一下"
Agent: [changes horizontal too, now vertical is wrong again]
... 20 more rounds ...
```

**After:**
```
/ui-iteration
Agent: "What needs to change? Let me build a spec."
User: "底部胶囊的间距和图标大小"
Agent: [builds spec: 3 properties to adjust]
Agent: [changes property 1, screenshots, confirms]
Agent: [changes property 2, screenshots, confirms]
Agent: [changes property 3, screenshots, confirms]
Done in 3 rounds.
```

## When to trigger

- `/ui-iteration` — start the structured UI adjustment loop
- User sends a screenshot with complaints about spacing, sizing, colors, alignment
- Multiple rounds of "还是不对", "间距不对", "太大/太小" style feedback
- High-fidelity cloning from a reference app or design mockup
- Any time you've made 3+ UI adjustments and the user is still not satisfied

## When NOT to use

- New feature implementation (layout, components) → just build it
- Functional bugs (button doesn't work, data not loading) → use `systematic-debugging`
- Color theme / dark mode issues → direct fix, no loop needed

## Core principles

### 1. One property per iteration

Never change multiple visual properties at once. If the user says "padding is wrong and the icon is too small", that's two iterations, not one. Changing both at once means you can't tell which change fixed (or broke) the layout.

### 2. Use design tokens, not magic numbers

When adjusting spacing, prefer values from an established scale rather than arbitrary numbers:

**Spacing scale (4px base):**
```
0, 2, 4, 6, 8, 10, 12, 16, 20, 24, 28, 32, 40, 48, 64
```

When the user says "a bit more padding", move ONE step up the scale, not +2px.

### 3. Always read before editing

Before changing a UI value, read the component to understand:
- What other components share this value (changing it may break siblings)
- Whether the value comes from a theme/token or is hardcoded
- The component hierarchy (is this padding on the parent or the child?)

## Workflow

### Step 1 — Build the adjustment spec

When the user describes what's wrong, extract a structured spec:

```
UI Adjustment Spec:
  Component: ServerSwitcherModal
  File: src/components/ServerSwitcher.tsx

  Adjustments:
    1. [SPACING] top padding: currently 16, user wants "more breathing room"
       → Try: 24 (next scale step up)
    2. [SIZE] close icon: currently 18, user says "too thin"
       → Try: 22 (next size step)
    3. [ALIGNMENT] empty state text: currently top-aligned, user wants centered
       → Try: flex justify-center
```

Present the spec to the user for confirmation before making any changes.

### Step 2 — Apply one change at a time

For each adjustment in the spec:

1. **Read** the current file and identify the exact property
2. **Change** only that one property
3. **Verify** — if possible, take a screenshot or describe the expected visual result
4. **Confirm** with the user before moving to the next adjustment

```
Applied adjustment 1/3: top padding 16 → 24

Next: adjustment 2 (close icon size). Proceed?
```

### Step 3 — Handle user feedback

When the user says the adjustment is wrong:

**"Too much" / "太大了"**: Move one scale step DOWN from the attempted value
**"Too little" / "太小了"**: Move one scale step UP from the attempted value
**"Not what I meant"**: Ask for clarification, update the spec
**"Perfect" / accepted**: Mark adjustment as done, move to next

### Step 4 — Reference-based iteration

When cloning from a reference app or screenshot:

1. **Study the reference** thoroughly before making any changes:
   ```bash
   # If reference is a file in the repo
   Read /Users/mark/MyProjects/iOSApp/UniClipboard/Views/ServerSwitcherView.swift
   
   # If reference is a screenshot
   # Read the image file for visual analysis
   Read /tmp/reference-screenshot.png
   ```

2. **Build a Transfer Spec** mapping reference → implementation:
   ```
   Reference → Implementation mapping:
     iOS .presentationDetents([.medium]) → snapPoints={['50%']}
     iOS .padding(.horizontal, 24) → paddingHorizontal: 24
     iOS .font(.body) → fontSize: 17 (iOS body = 17pt)
     iOS .foregroundStyle(.secondary) → color: theme.textSecondary
   ```

3. **Apply each mapping one at a time**, verifying after each

### Step 5 — Platform-specific considerations

#### iOS (Expo)
- Use `@expo/ui` components for native feel when available
- iOS system font sizes: caption2=11, caption=12, footnote=13, subheadline=15, body=17, title3=20, title2=22, title1=28, largeTitle=34
- Standard iOS margins: 16 (compact), 20 (regular)
- Use SafeAreaView for edge-to-edge layouts

#### Android (Expo)
- Material Design 3 spacing: 4, 8, 12, 16, 24, 32
- Use elevation for depth instead of shadows
- Bottom sheet: use `@gorhom/bottom-sheet` with proper Android styling

#### React (Tauri desktop)
- Tailwind spacing scale: 0.5=2px, 1=4px, 1.5=6px, 2=8px, 3=12px, 4=16px, 5=20px, 6=24px, 8=32px
- Use CSS variables from the project's theme
- Check both light and dark mode after changes

## Convergence strategy

If after 3 rounds of adjusting the same property the user is still not satisfied:

```
We've adjusted top padding three times (16 → 24 → 20 → 22).
Let me ask more precisely:

What's the visual effect you're aiming for?
  A) Match the iOS Settings app spacing
  B) Equal spacing top and sides
  C) Specific pixel value you have in mind
  D) Show me a reference screenshot
```

Getting explicit alignment on the target prevents further oscillation.

## Layout consistency check

After all adjustments are made, do a quick consistency audit:

```bash
# Check if the adjusted values are consistent with sibling components
grep -n "padding\|margin\|gap\|spacing" <file> | head -20
```

If the same component family uses different padding values after adjustments, flag it:

```
⚠️ Consistency note:
  ServerSwitcherModal: paddingHorizontal=24
  AddServerSheet: paddingHorizontal=16
  
  These are sibling sheets — should they use the same value?
```

## Anti-patterns

- Changing 3+ properties at once ("I'll fix padding, margin, and font size together")
- Using magic numbers instead of scale values (17px padding, 13px gap)
- Not reading the component before editing (missing shared styles, theme tokens)
- Oscillating between two values without asking for clarification
- Applying iOS design values to Android without adaptation
- Making spacing changes without checking dark mode / different screen sizes
- Spending 10 rounds on one property without asking the user for a reference
- Changing the component's structure/layout when only spacing was requested
