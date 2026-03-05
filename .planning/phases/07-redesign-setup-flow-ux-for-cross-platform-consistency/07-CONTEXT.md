# Phase 7: Redesign setup flow UX for cross-platform consistency - Context

**Gathered:** 2026-03-05
**Status:** Ready for planning

<domain>
## Phase Boundary

Redesign the setup flow frontend (SetupPage + all step components) to achieve consistent UX across Windows, macOS, and Linux. Unified spacing, font sizing, adaptive layout, and coherent animations. Frontend-only — no backend changes.

</domain>

<decisions>
## Implementation Decisions

### Step Layout Unification

- Extract a shared `StepLayout` component with 4 slots: headerLeft/headerRight, title+subtitle, children (content area), footer (action buttons + hint text)
- StepLayout supports `variant='centered'` mode for PairingConfirmStep and SetupDoneStep
- WelcomeStep stays independent (no back button, unique dual-card layout)
- StepLayout includes a unified error display slot (fade-in animation with AlertCircle icon)
- Each Step internally resolves its Rust enum error type to an i18n string, then passes the string to StepLayout's error slot
- Extract ProcessingJoinSpace inline JSX (~35 lines in SetupPage) into a standalone `ProcessingJoinStep.tsx` component

### Cross-Platform Adaptive Strategy

- Use Tailwind viewport breakpoints for responsiveness
- Standardize on a single breakpoint: `sm:` only (640px) — remove all `lg:` usage from setup flow
- Replace any fixed px bracket values with rem equivalents (e.g., `min-h-[12rem]` is already rem, acceptable)
- Keep `max-w-3xl` (48rem) for content area max width

### Animation & Transitions

- Unified x-axis slide animation for all steps: forward = slide in from right, backward = slide in from left
- StepLayout controls animation via `direction` prop ('forward' | 'backward'), wrapping content in motion.div
- Individual Step components no longer manage their own motion.div wrappers
- Duration: 200ms, easing: ease-out
- Keep `AnimatePresence mode='wait'` — sequential transitions, no overlap

### Visual Style

- Add minimal step dot indicator at bottom center (filled dot = current step, hollow dots = other steps)
- Keep existing security badges (E2EE, Local Keys, LAN Discovery) in bottom-right corner, adjust breakpoint from `lg:` to `sm:`
- WelcomeStep: change from horizontal dual-card layout to vertical (always stacked), cards become larger and more prominent

### Claude's Discretion

- Exact spacing values within StepLayout (gap sizes between slots)
- Step dot indicator implementation details (size, color, spacing)
- Whether ProcessingCreateSpace also gets extracted to its own component
- Exact animation easing curve parameters
- How to determine animation direction in SetupPage (state transition tracking)

</decisions>

<specifics>
## Specific Ideas

- User wants the experience to feel identical across Windows, macOS, and Linux — no platform-specific visual differences
- The setup flow should feel like a modern onboarding wizard with clear forward/backward navigation sense
- Welcome page cards should be vertically stacked for consistent rendering regardless of window width

</specifics>

<code_context>

## Existing Code Insights

### Reusable Assets

- `Button` component (`src/components/ui/button.tsx`): Already used across all steps, supports variants and sizes
- `Input` / `Label` components: Used in passphrase steps
- `Card` component (`src/components/ui/card.tsx`): Available but not currently used in setup flow
- `framer-motion`: Already a dependency, AnimatePresence and motion.div in use
- CSS variables for font sizes (`--font-size-sm/base/lg`) in globals.css
- Glass utility classes (`glass-weak`, `glass`, `glass-strong`) available

### Established Patterns

- Theme system: CSS variables for colors, multiple theme support (zinc, catppuccin, t3chat, claude)
- Font stack: `-apple-system, BlinkMacSystemFont, 'Segoe UI', 'Microsoft YaHei UI', system-ui, sans-serif`
- i18n: All text uses `useTranslation` with keyPrefix pattern
- Error types: Rust enums passed via props, resolved to i18n strings in each component
- State management: SetupPage manages all state, steps are presentational components with callback props

### Integration Points

- `SetupPage.tsx` is the orchestrator — routes state to step components via `renderStep()`
- `types.ts` defines all step prop interfaces extending `StepProps` base
- `SetupFlow.test.tsx` has existing tests that mock framer-motion — new StepLayout must be compatible
- `stepKey` useMemo in SetupPage drives AnimatePresence key changes

</code_context>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

_Phase: 07-redesign-setup-flow-ux-for-cross-platform-consistency_
_Context gathered: 2026-03-05_
