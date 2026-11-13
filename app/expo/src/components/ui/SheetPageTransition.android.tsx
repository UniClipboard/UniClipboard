import {
  AnimatedVisibility,
  Box,
  Column,
  EnterTransition,
  ExitTransition,
} from '@expo/ui/jetpack-compose';
import { animateContentSize, fillMaxWidth } from '@expo/ui/jetpack-compose/modifiers';

import type { SheetPageTransitionProps } from './SheetPageTransition.types';

const FIRST_PAGE_ENTER_TRANSITION = EnterTransition.fadeIn().plus(
  EnterTransition.slideInHorizontally({ initialOffsetX: -0.12 })
);
const FIRST_PAGE_EXIT_TRANSITION = ExitTransition.fadeOut().plus(
  ExitTransition.slideOutHorizontally({ targetOffsetX: -0.12 })
);
const SECOND_PAGE_ENTER_TRANSITION = EnterTransition.fadeIn().plus(
  EnterTransition.slideInHorizontally({ initialOffsetX: 0.12 })
);
const SECOND_PAGE_EXIT_TRANSITION = ExitTransition.fadeOut().plus(
  ExitTransition.slideOutHorizontally({ targetOffsetX: 0.12 })
);

/**
 * Keeps drill-down navigation inside one Android bottom sheet, with a directional transition.
 */
export function SheetPageTransition({
  showSecondPage,
  firstPage,
  secondPage,
}: SheetPageTransitionProps) {
  return (
    <Column modifiers={[fillMaxWidth(), animateContentSize()]}>
      <Box modifiers={[fillMaxWidth()]}>
        <AnimatedVisibility
          visible={!showSecondPage}
          enterTransition={FIRST_PAGE_ENTER_TRANSITION}
          exitTransition={FIRST_PAGE_EXIT_TRANSITION}
          modifiers={[fillMaxWidth()]}
        >
          {firstPage}
        </AnimatedVisibility>

        <AnimatedVisibility
          visible={showSecondPage}
          enterTransition={SECOND_PAGE_ENTER_TRANSITION}
          exitTransition={SECOND_PAGE_EXIT_TRANSITION}
          modifiers={[fillMaxWidth()]}
        >
          {secondPage}
        </AnimatedVisibility>
      </Box>
    </Column>
  );
}
