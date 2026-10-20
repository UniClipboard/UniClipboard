# History detail UI regression repair

## Phases
1. Reproduce loading flash, image fact reflow and text clipping using production components in Chromium for linux/macos/windows (complete).
2. Fix shared data lifecycles and preview layout ownership (complete).
3. Repeat identical browser checks, run targeted tests, typecheck and React Doctor (complete).

## Next Step
Complete: report acceptance results and artifact locations.

## Acceptance
Previously visited entries do not show loading or lose delivery rows; image fact positions remain stable during delayed metadata; the final line of long text is reachable; switching selections resets scrolling.
