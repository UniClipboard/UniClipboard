# Phase 7: Redesign Setup Flow UX for Cross-Platform Consistency - Research

**Researched:** 2026-03-05
**Domain:** React component architecture, Framer Motion animations, Tailwind CSS responsive design
**Confidence:** HIGH

## Summary

This phase is a frontend-only refactoring of the setup flow (SetupPage + 6 step components) to achieve visual consistency across Windows, macOS, and Linux. The core work involves extracting a shared `StepLayout` component, unifying slide animations, changing WelcomeStep to vertical card layout, adding step dot indicators, and standardizing breakpoints to `sm:` only.

The existing codebase already uses framer-motion (v12.23.26), Tailwind CSS v4, React 18, and Vitest -- no new dependencies needed. Each step currently manages its own `motion.div` wrapper with inconsistent animations (some use x-axis slide, others use opacity+scale). The refactoring centralizes animation control in StepLayout and removes per-step motion wrappers.

**Primary recommendation:** Extract StepLayout as the single structural component for all steps except WelcomeStep. Move animation ownership from individual steps to StepLayout. Track transition direction in SetupPage by comparing previous and current state ordinals.

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

- Extract a shared `StepLayout` component with 4 slots: headerLeft/headerRight, title+subtitle, children (content area), footer (action buttons + hint text)
- StepLayout supports `variant='centered'` mode for PairingConfirmStep and SetupDoneStep
- WelcomeStep stays independent (no back button, unique dual-card layout)
- StepLayout includes a unified error display slot (fade-in animation with AlertCircle icon)
- Each Step internally resolves its Rust enum error type to an i18n string, then passes the string to StepLayout's error slot
- Extract ProcessingJoinSpace inline JSX (~35 lines in SetupPage) into a standalone `ProcessingJoinStep.tsx` component
- Use Tailwind viewport breakpoints for responsiveness
- Standardize on a single breakpoint: `sm:` only (640px) -- remove all `lg:` usage from setup flow
- Replace any fixed px bracket values with rem equivalents
- Keep `max-w-3xl` (48rem) for content area max width
- Unified x-axis slide animation for all steps: forward = slide in from right, backward = slide in from left
- StepLayout controls animation via `direction` prop ('forward' | 'backward'), wrapping content in motion.div
- Individual Step components no longer manage their own motion.div wrappers
- Duration: 200ms, easing: ease-out
- Keep `AnimatePresence mode='wait'` -- sequential transitions, no overlap
- Add minimal step dot indicator at bottom center (filled dot = current step, hollow dots = other steps)
- Keep existing security badges (E2EE, Local Keys, LAN Discovery) in bottom-right corner, adjust breakpoint from `lg:` to `sm:`
- WelcomeStep: change from horizontal dual-card layout to vertical (always stacked), cards become larger and more prominent

### Claude's Discretion

- Exact spacing values within StepLayout (gap sizes between slots)
- Step dot indicator implementation details (size, color, spacing)
- Whether ProcessingCreateSpace also gets extracted to its own component
- Exact animation easing curve parameters
- How to determine animation direction in SetupPage (state transition tracking)

### Deferred Ideas (OUT OF SCOPE)

None -- discussion stayed within phase scope

</user_constraints>

## Standard Stack

### Core

| Library                | Version     | Purpose                                 | Why Standard               |
| ---------------------- | ----------- | --------------------------------------- | -------------------------- |
| react                  | ^18.3.1     | UI framework                            | Already in use             |
| framer-motion          | ^12.23.26   | Animation (AnimatePresence, motion.div) | Already in use, mature API |
| tailwindcss            | ^4.1.18     | Utility-first CSS                       | Already in use, v4         |
| vitest                 | ^4.0.17     | Test framework                          | Already configured         |
| @testing-library/react | (installed) | Component testing                       | Already in use             |

### Supporting

| Library       | Version     | Purpose                              | When to Use      |
| ------------- | ----------- | ------------------------------------ | ---------------- |
| lucide-react  | (installed) | Icons (ArrowLeft, AlertCircle, etc.) | All step UI      |
| react-i18next | (installed) | Translations                         | All text content |
| sonner        | (installed) | Toast notifications                  | Error feedback   |

### Alternatives Considered

None -- all dependencies are already installed. No new packages needed.

## Architecture Patterns

### Recommended Project Structure

```
src/pages/setup/
├── StepLayout.tsx          # NEW: shared layout component
├── StepDotIndicator.tsx    # NEW: step progress dots
├── ProcessingJoinStep.tsx  # NEW: extracted from SetupPage inline JSX
├── WelcomeStep.tsx         # MODIFIED: vertical card layout
├── CreatePassphraseStep.tsx # MODIFIED: use StepLayout, remove motion.div
├── JoinPickDeviceStep.tsx   # MODIFIED: use StepLayout, remove motion.div
├── JoinVerifyPassphraseStep.tsx # MODIFIED: use StepLayout, remove motion.div
├── PairingConfirmStep.tsx   # MODIFIED: use StepLayout variant='centered'
├── SetupDoneStep.tsx        # MODIFIED: use StepLayout variant='centered'
├── types.ts                 # MODIFIED: add StepLayout types
└── __tests__/
    ├── SetupFlow.test.tsx           # EXISTING: in parent __tests__/
    ├── StepLayout.test.tsx          # NEW
    ├── welcome-layout.test.tsx      # EXISTING
    └── ...
```

### Pattern 1: StepLayout Component Architecture

**What:** A slot-based layout component that handles structure, animation, and error display for all non-Welcome steps.

**When to use:** Every step except WelcomeStep wraps its content in StepLayout.

**Example:**

```typescript
// StepLayout props interface
interface StepLayoutProps {
  // Header slots
  headerLeft?: ReactNode // Back button
  headerRight?: ReactNode // Refresh button (JoinPickDevice only)

  // Title section
  title: string
  subtitle?: string

  // Content
  children: ReactNode

  // Footer
  footer?: ReactNode // Action buttons
  hint?: string // Hint text below footer

  // Error display
  error?: string | null // Resolved i18n string (not raw Rust enum)

  // Layout variant
  variant?: 'default' | 'centered'

  // Animation
  direction?: 'forward' | 'backward'
}
```

**Key design decision:** Error is passed as a pre-resolved i18n string, NOT as the raw Rust enum. Each step is responsible for mapping its error type to a translated string, then passes it to StepLayout.

### Pattern 2: Animation Direction Tracking

**What:** SetupPage tracks transition direction by comparing state ordinals.

**Recommended approach:** Assign ordinal values to each state and compare previous vs current.

```typescript
// State ordinal mapping for direction inference
function getStateOrdinal(state: SetupState | null): number {
  if (!state) return -1
  if (state === 'Welcome') return 0
  if (state === 'Completed') return 99
  if (typeof state === 'object') {
    if ('CreateSpaceInputPassphrase' in state) return 1
    if ('ProcessingCreateSpace' in state) return 2
    if ('JoinSpaceSelectDevice' in state) return 1
    if ('JoinSpaceInputPassphrase' in state) return 2
    if ('JoinSpaceConfirmPeer' in state) return 3
    if ('ProcessingJoinSpace' in state) return 4
  }
  return 0
}

// In SetupPage: track previous state with useRef
const prevStateRef = useRef<SetupState | null>(null)
const direction =
  getStateOrdinal(setupState) >= getStateOrdinal(prevStateRef.current) ? 'forward' : 'backward'

// Update ref after render
useEffect(() => {
  prevStateRef.current = setupState
}, [setupState])
```

### Pattern 3: Step Dot Indicator

**What:** Minimal progress indicator showing current position in the setup flow.

**Recommended:** Simple array of filled/hollow circles at the bottom center of the page, outside the scrollable content area.

```typescript
// Step indices for dot indicator
// Welcome=0, Create/Join steps=1..N, Done=last
// Only show dots when NOT on Welcome (Welcome has no dots)
interface StepDotIndicatorProps {
  totalSteps: number
  currentStep: number
}
```

### Anti-Patterns to Avoid

- **Per-step motion.div wrappers:** Steps MUST NOT wrap in their own motion.div. StepLayout owns the animation wrapper.
- **lg: breakpoints in setup flow:** User explicitly banned `lg:` -- use `sm:` only.
- **Fixed px values in brackets:** Use rem equivalents (`min-h-[12rem]` is fine, `min-w-[80px]` is not).
- **Raw Rust error enums in StepLayout:** Each step resolves to i18n string before passing to the error slot.

## Don't Hand-Roll

| Problem                | Don't Build             | Use Instead                                | Why                                               |
| ---------------------- | ----------------------- | ------------------------------------------ | ------------------------------------------------- |
| Entry/exit animations  | Manual CSS transitions  | framer-motion AnimatePresence + motion.div | Handles mount/unmount animation correctly         |
| Responsive breakpoints | Media queries in CSS    | Tailwind `sm:` prefix                      | Consistent with project conventions               |
| Icon library           | SVG inline              | lucide-react                               | Already used throughout, tree-shakeable           |
| Step progress dots     | Complex stepper library | Simple div with conditional fill           | Only 5-8 dots, no complex stepper behavior needed |

## Common Pitfalls

### Pitfall 1: AnimatePresence Key Stability

**What goes wrong:** If the `key` prop on the animated child doesn't change when the step changes, AnimatePresence won't trigger exit/enter animations.
**Why it happens:** The existing `stepKey` useMemo derives key from `setupState`. If the state object shape is the same but values differ (e.g., error changes), no animation fires.
**How to avoid:** The existing `stepKey` logic is correct (uses `Object.keys(setupState)[0]`). Keep it. Don't change key on error-only updates.
**Warning signs:** Step transitions happen without animation.

### Pitfall 2: Animation Direction on Cancel/Back

**What goes wrong:** When user clicks "Back," the new step slides in from the right (forward direction) instead of the left (backward).
**Why it happens:** Direction must be computed BEFORE the state updates, but React state updates are asynchronous.
**How to avoid:** Use useRef to store previous state ordinal. Compute direction by comparing previous ordinal to new ordinal. The ref updates in useEffect after render, so by the time the next transition starts, prevState is correctly the "before" value.
**Warning signs:** All transitions slide the same direction regardless of Back/Forward action.

### Pitfall 3: Test Compatibility with StepLayout

**What goes wrong:** Existing tests in `SetupFlow.test.tsx` mock `framer-motion` globally. If StepLayout uses motion.div internally, tests need the mock to pass motion props through.
**Why it happens:** The existing mock replaces `motion.div` with a plain `<div>` that spreads all props. This should work as long as StepLayout doesn't rely on framer-motion runtime behavior in tests.
**How to avoid:** Keep the existing framer-motion mock pattern. StepLayout's animation behavior is visual-only and doesn't need testing. Test structure and content rendering.
**Warning signs:** Tests crash with "cannot read property of undefined" from framer-motion internals.

### Pitfall 4: WelcomeStep Overflow Behavior

**What goes wrong:** WelcomeStep with vertical cards can overflow the viewport on small screens.
**Why it happens:** Two stacked cards are taller than side-by-side cards. The `main` element needs different overflow behavior per step.
**How to avoid:** The working tree already has a test expecting `overflow-hidden` on `<main>` for Welcome step (non-scrollable since cards should fit). Other steps use `overflow-y-auto` for scrollable content. SetupPage needs conditional overflow class based on `stepKey`.
**Warning signs:** Cards get cut off or page scrolls unexpectedly.

### Pitfall 5: Tailwind v4 Class Conflicts

**What goes wrong:** Tailwind v4 handles class conflicts differently than v3.
**Why it happens:** The project uses `tailwind-merge` (v3.4.0) via the `cn()` utility. When composing classes in StepLayout, ensure `cn()` is used for any dynamic class merging to avoid specificity issues.
**How to avoid:** Always use `cn()` from `@/lib/utils` when conditionally applying classes.

## Code Examples

### StepLayout Skeleton

```typescript
// src/pages/setup/StepLayout.tsx
import { motion } from 'framer-motion'
import { AlertCircle } from 'lucide-react'
import { type ReactNode } from 'react'

interface StepLayoutProps {
  headerLeft?: ReactNode
  headerRight?: ReactNode
  title: string
  subtitle?: string
  children: ReactNode
  footer?: ReactNode
  hint?: string
  error?: string | null
  variant?: 'default' | 'centered'
  direction?: 'forward' | 'backward'
}

const slideVariants = {
  enter: (direction: 'forward' | 'backward') => ({
    x: direction === 'forward' ? 20 : -20,
    opacity: 0,
  }),
  center: {
    x: 0,
    opacity: 1,
  },
  exit: (direction: 'forward' | 'backward') => ({
    x: direction === 'forward' ? -20 : 20,
    opacity: 0,
  }),
}

export default function StepLayout({
  headerLeft,
  headerRight,
  title,
  subtitle,
  children,
  footer,
  hint,
  error,
  variant = 'default',
  direction = 'forward',
}: StepLayoutProps) {
  const isCentered = variant === 'centered'

  return (
    <motion.div
      custom={direction}
      variants={slideVariants}
      initial="enter"
      animate="center"
      exit="exit"
      transition={{ duration: 0.2, ease: 'easeOut' }}
      className="w-full"
    >
      {/* Header row */}
      {(headerLeft || headerRight) && (
        <div className="mb-5 flex items-center justify-between sm:mb-6">
          {headerLeft ?? <div />}
          {headerRight}
        </div>
      )}

      {/* Title section */}
      <div className={`mb-6 sm:mb-8 ${isCentered ? 'text-center' : ''}`}>
        <h1 className="text-2xl font-semibold tracking-tight text-foreground">
          {title}
        </h1>
        {subtitle && (
          <p className="mt-2 text-muted-foreground">{subtitle}</p>
        )}
      </div>

      {/* Content */}
      {children}

      {/* Error display */}
      {error && (
        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          className={`mt-4 flex items-center gap-2 text-sm text-destructive ${isCentered ? 'justify-center' : ''}`}
        >
          <AlertCircle className="h-4 w-4 shrink-0" />
          {error}
        </motion.div>
      )}

      {/* Footer */}
      {footer && (
        <div className={`mt-7 sm:mt-8 ${isCentered ? 'flex justify-center' : ''}`}>
          {footer}
        </div>
      )}

      {/* Hint */}
      {hint && (
        <p className="mt-4 text-xs text-muted-foreground sm:mt-5">{hint}</p>
      )}
    </motion.div>
  )
}
```

### Step Migration Example (CreatePassphraseStep)

```typescript
// Before: manages own motion.div, own error display, own back button layout
// After: delegates to StepLayout

export default function CreatePassphraseStep({ onSubmit, onBack, error, loading }) {
  const { t } = useTranslation(undefined, { keyPrefix: 'setup.createPassphrase' })
  const [localError, setLocalError] = useState<string | null>(null)

  // Error resolution stays in the step (maps Rust enum to i18n string)
  useEffect(() => { /* same error mapping logic */ }, [error, t])

  const resolvedError = localError // already an i18n string

  return (
    <StepLayout
      headerLeft={<BackButton onClick={onBack} />}
      title={t('title')}
      subtitle={t('subtitle')}
      error={resolvedError}
      footer={
        <Button onClick={handleSubmit} disabled={loading} className="min-w-32">
          {loading ? <><Loader2 className="mr-2 h-4 w-4 animate-spin" />{t('actions.creating')}</> : t('actions.submit')}
        </Button>
      }
      hint={t('hint')}
    >
      {/* Just the form fields -- no wrapper, no motion.div */}
      <div className="space-y-6">
        {/* passphrase inputs */}
      </div>
    </StepLayout>
  )
}
```

### WelcomeStep Vertical Layout

```typescript
// Change from: grid grid-cols-1 sm:grid-cols-2 (horizontal on sm+)
// Change to: flex flex-col (always vertical)
<div className="flex flex-col gap-4">
  <button className="group relative flex flex-col items-start gap-5 rounded-xl border bg-card p-7 text-left shadow-sm ...">
    {/* Card content -- larger padding, more prominent */}
  </button>
  <button className="group relative flex flex-col items-start gap-5 rounded-xl border bg-card p-7 text-left shadow-sm ...">
    {/* Card content */}
  </button>
</div>
```

## State of the Art

| Old Approach                       | Current Approach                       | When Changed | Impact                                |
| ---------------------------------- | -------------------------------------- | ------------ | ------------------------------------- |
| Per-step motion.div wrappers       | Centralized StepLayout animation       | This phase   | Consistent animation behavior         |
| `sm:grid-cols-2` for Welcome cards | Always vertical stack                  | This phase   | Cross-platform consistency            |
| `lg:` breakpoints in setup flow    | `sm:` only                             | This phase   | Single breakpoint, simpler responsive |
| Inline ProcessingJoinSpace JSX     | Extracted ProcessingJoinStep component | This phase   | Cleaner SetupPage orchestrator        |

## Validation Architecture

### Test Framework

| Property           | Value                                                                                                  |
| ------------------ | ------------------------------------------------------------------------------------------------------ |
| Framework          | Vitest 4.x + @testing-library/react                                                                    |
| Config file        | `vite.config.ts` (test section)                                                                        |
| Quick run command  | `bunx vitest run src/pages/__tests__/SetupFlow.test.tsx src/pages/setup/__tests__/ --reporter=verbose` |
| Full suite command | `bunx vitest run --reporter=verbose`                                                                   |

### Phase Requirements to Test Map

| Req ID | Behavior                                          | Test Type  | Automated Command                                                                  | File Exists?       |
| ------ | ------------------------------------------------- | ---------- | ---------------------------------------------------------------------------------- | ------------------ |
| UX-01  | StepLayout renders 4 slots correctly              | unit       | `bunx vitest run src/pages/setup/__tests__/StepLayout.test.tsx -x`                 | No -- Wave 0       |
| UX-02  | StepLayout centered variant                       | unit       | `bunx vitest run src/pages/setup/__tests__/StepLayout.test.tsx -x`                 | No -- Wave 0       |
| UX-03  | WelcomeStep vertical card layout                  | unit       | `bunx vitest run src/pages/setup/__tests__/welcome-layout.test.tsx -x`             | Yes (needs update) |
| UX-04  | Step dot indicator renders correct count/position | unit       | `bunx vitest run src/pages/setup/__tests__/StepDotIndicator.test.tsx -x`           | No -- Wave 0       |
| UX-05  | No lg: breakpoints in setup flow                  | smoke      | `grep -r "lg:" src/pages/setup/ src/pages/SetupPage.tsx` (should return 0 matches) | Manual             |
| UX-06  | Animation direction changes on back vs forward    | unit       | `bunx vitest run src/pages/__tests__/SetupFlow.test.tsx -x`                        | Yes (needs update) |
| UX-07  | ProcessingJoinStep extracted and renders          | unit       | `bunx vitest run src/pages/setup/__tests__/ProcessingJoinStep.test.tsx -x`         | No -- Wave 0       |
| UX-08  | Existing tests still pass after refactor          | regression | `bunx vitest run src/pages/ --reporter=verbose -x`                                 | Yes                |

### Sampling Rate

- **Per task commit:** `bunx vitest run src/pages/ --reporter=verbose`
- **Per wave merge:** `bunx vitest run --reporter=verbose`
- **Phase gate:** Full suite green before verification

### Wave 0 Gaps

- [ ] `src/pages/setup/__tests__/StepLayout.test.tsx` -- covers UX-01, UX-02
- [ ] `src/pages/setup/__tests__/StepDotIndicator.test.tsx` -- covers UX-04
- [ ] `src/pages/setup/__tests__/ProcessingJoinStep.test.tsx` -- covers UX-07

## Working Tree State

**Important:** The working tree has uncommitted changes from the context gathering session. These changes are minor tweaks to spacing/sizing (sm: responsive adjustments) across all step components and SetupPage. The phase implementation should incorporate these changes rather than reverting them. The test file also has a new test (`uses non-scrollable main layout on welcome step`) that will need to be addressed by the implementation.

### Files with uncommitted changes:

- `src/pages/SetupPage.tsx` -- ProcessingJoinSpace responsive tweaks, max-w-3xl, min-h-0 flex layout
- `src/pages/setup/WelcomeStep.tsx` -- minor spacing adjustments
- `src/pages/setup/CreatePassphraseStep.tsx` -- minor spacing
- `src/pages/setup/JoinPickDeviceStep.tsx` -- minor spacing
- `src/pages/setup/JoinVerifyPassphraseStep.tsx` -- minor spacing
- `src/pages/setup/PairingConfirmStep.tsx` -- minor spacing
- `src/pages/setup/SetupDoneStep.tsx` -- minor spacing
- `src/pages/__tests__/SetupFlow.test.tsx` -- new overflow-hidden test for Welcome

## Open Questions

1. **ProcessingCreateSpace extraction**
   - What we know: Currently a simple inline loading display (~8 lines) in SetupPage
   - What's unclear: Whether it warrants its own component file
   - Recommendation: Extract to `ProcessingCreateStep.tsx` for consistency with ProcessingJoinStep. The overhead is minimal and it completes the "every state has a component" pattern.

2. **Step dot count for branching flows**
   - What we know: Create flow has ~3 steps, Join flow has ~5 steps. The total dot count depends on which path the user chose.
   - What's unclear: Whether to show max dots for both flows or dynamic per-flow
   - Recommendation: Dynamic per-flow. After Welcome, determine flow (create vs join) and show appropriate dot count. Welcome and Done are shared.

3. **Overflow handling per step**
   - What we know: WelcomeStep needs `overflow-hidden` (cards must fit viewport). Other steps need `overflow-y-auto` (forms may overflow on small screens).
   - What's unclear: Best way to conditionally apply overflow class on `<main>`
   - Recommendation: Compute overflow class in SetupPage based on `stepKey === 'Welcome'`.

## Sources

### Primary (HIGH confidence)

- Project source code -- all step components, SetupPage, types, tests
- CONTEXT.md -- locked user decisions
- package.json -- dependency versions confirmed

### Secondary (MEDIUM confidence)

- framer-motion custom variants pattern -- well-documented in framer-motion docs, commonly used for directional transitions
- Tailwind v4 breakpoint behavior -- consistent with v3 for `sm:` prefix usage

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH -- all dependencies already installed, no new packages
- Architecture: HIGH -- clear user decisions, straightforward component extraction
- Pitfalls: HIGH -- identified from direct code analysis of existing implementation

**Research date:** 2026-03-05
**Valid until:** 2026-04-05 (stable -- frontend refactoring, no moving targets)
