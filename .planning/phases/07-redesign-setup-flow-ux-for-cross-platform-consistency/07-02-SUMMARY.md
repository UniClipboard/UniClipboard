---
phase: 07-redesign-setup-flow-ux-for-cross-platform-consistency
plan: 02
subsystem: ui
tags: [react, framer-motion, tailwindcss, step-layout, animation, responsive]

# Dependency graph
requires:
  - phase: 07-01
    provides: StepLayout, StepDotIndicator, ProcessingJoinStep foundation components
provides:
  - All step components migrated to StepLayout (unified layout/animation)
  - WelcomeStep with horizontal card layout and directional animation
  - SetupPage with direction tracking, dot indicator, overflow handling
  - Zero lg: breakpoints across entire setup flow
affects: []

# Tech tracking
tech-stack:
  added: []
  patterns:
    - 'StepLayout slot pattern: headerLeft, headerRight, title, subtitle, children, footer, hint, error'
    - 'Direction-aware animation: getStateOrdinal compares previous vs current state for forward/backward'
    - 'Step dot indicator computed from flow type (create=3, join=5 steps)'

key-files:
  created: []
  modified:
    - src/pages/setup/WelcomeStep.tsx
    - src/pages/setup/CreatePassphraseStep.tsx
    - src/pages/setup/JoinPickDeviceStep.tsx
    - src/pages/setup/JoinVerifyPassphraseStep.tsx
    - src/pages/setup/PairingConfirmStep.tsx
    - src/pages/setup/SetupDoneStep.tsx
    - src/pages/SetupPage.tsx
    - src/pages/setup/types.ts
    - src/pages/setup/StepLayout.tsx
    - src/pages/__tests__/SetupFlow.test.tsx

key-decisions:
  - "Kept horizontal (flex-row) WelcomeStep card layout after visual verification (overrides plan's flex-col)"
  - 'Removed security badges entirely after visual review (cluttered small windows)'
  - 'Increased StepLayout header spacing for better visual hierarchy'

patterns-established:
  - 'Step migration pattern: remove motion.div/AlertCircle, pass slots to StepLayout'
  - 'Direction tracking via state ordinal comparison with prevStateRef'

requirements-completed: [UX-03, UX-05, UX-06, UX-08]

# Metrics
duration: 15min
completed: 2026-03-05
---

# Phase 07 Plan 02: Step Component Migration Summary

**Migrated all 5 non-Welcome steps to StepLayout with directional animations, added dot indicator and overflow handling to SetupPage, eliminated all lg: breakpoints**

## Performance

- **Duration:** 15 min
- **Started:** 2026-03-05T09:35:00Z
- **Completed:** 2026-03-05T09:50:00Z
- **Tasks:** 3 (2 auto + 1 visual verification checkpoint)
- **Files modified:** 10

## Accomplishments

- All 5 non-Welcome step components (CreatePassphrase, JoinPickDevice, JoinVerifyPassphrase, PairingConfirm, SetupDone) migrated to StepLayout
- SetupPage updated with direction tracking (forward/backward via state ordinals), StepDotIndicator, overflow-hidden for Welcome / overflow-y-auto for other steps
- ProcessingJoinSpace inline JSX replaced with ProcessingJoinStep component
- Zero lg: breakpoints across all setup flow files (sm: only for cross-platform consistency)
- All 38 tests pass (6 SetupFlow + 12 StepLayout + 3 StepDotIndicator + others)

## Task Commits

Each task was committed atomically:

1. **Task 1: Migrate all step components to StepLayout and refactor WelcomeStep** - `f3fe788` (feat)
2. **Task 2: Update SetupPage with direction tracking, dot indicator, overflow, badges, and wiring** - `dbaf15c` (feat)
3. **Task 3: Visual verification** - Passed via UAT `01afe1b`

Post-verification fixes:

- `19a4976` - fix(07-02): always use horizontal layout for WelcomeStep cards
- `a3def5c` - fix(07-02): remove security badges and increase header spacing in StepLayout

## Files Created/Modified

- `src/pages/setup/WelcomeStep.tsx` - Horizontal card layout with directional slide animation
- `src/pages/setup/CreatePassphraseStep.tsx` - Migrated to StepLayout with headerLeft/footer/hint/error slots
- `src/pages/setup/JoinPickDeviceStep.tsx` - Migrated to StepLayout with headerLeft/headerRight slots
- `src/pages/setup/JoinVerifyPassphraseStep.tsx` - Migrated both render paths (normal + mismatchHelp) to StepLayout
- `src/pages/setup/PairingConfirmStep.tsx` - Migrated to StepLayout variant='centered' with resolved error
- `src/pages/setup/SetupDoneStep.tsx` - Migrated to StepLayout variant='centered' with footer
- `src/pages/SetupPage.tsx` - Direction tracking, StepDotIndicator, overflow handling, ProcessingJoinStep wiring
- `src/pages/setup/types.ts` - Added direction prop to StepProps
- `src/pages/setup/StepLayout.tsx` - Increased header spacing
- `src/pages/__tests__/SetupFlow.test.tsx` - Added dot indicator and overflow tests

## Decisions Made

- Kept horizontal (flex-row) WelcomeStep card layout after visual verification -- vertical felt cramped on wide screens
- Removed security badges from SetupPage after visual review -- they cluttered the UI on small windows without adding value
- Increased StepLayout header margin from mb-4 to mb-6 for better visual hierarchy

## Deviations from Plan

### Post-Verification Adjustments

**1. [User Decision] Horizontal WelcomeStep cards instead of vertical**

- **Found during:** Task 3 (visual verification)
- **Issue:** Vertical card layout (flex-col) felt cramped; horizontal better utilizes screen width
- **Fix:** Changed flex-col back to flex-row
- **Commit:** `19a4976`

**2. [User Decision] Removed security badges**

- **Found during:** Task 3 (visual verification)
- **Issue:** Security badges (E2EE, Local Keys, LAN Discovery) cluttered the bottom of the setup page
- **Fix:** Removed badges section entirely, increased header spacing
- **Commit:** `a3def5c`

---

**Total deviations:** 2 (both user-directed after visual verification)
**Impact on plan:** Minor UI adjustments based on visual feedback. No functional impact.

## Issues Encountered

None

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Setup flow UX redesign is complete
- All components use consistent StepLayout pattern
- Cross-platform breakpoint consistency achieved (sm: only, no lg:)
- Ready for Phase 07 Plan 03 if applicable, or phase completion

---

_Phase: 07-redesign-setup-flow-ux-for-cross-platform-consistency_
_Completed: 2026-03-05_
