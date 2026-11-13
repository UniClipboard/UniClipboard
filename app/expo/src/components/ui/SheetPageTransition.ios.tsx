import type { SheetPageTransitionProps } from './SheetPageTransition.types';

export function SheetPageTransition({
  showSecondPage,
  firstPage,
  secondPage,
}: SheetPageTransitionProps) {
  return showSecondPage ? secondPage : firstPage;
}
