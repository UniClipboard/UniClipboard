import type { ReactNode } from 'react';

export interface SheetPageTransitionProps {
  showSecondPage: boolean;
  firstPage: ReactNode;
  secondPage: ReactNode;
}
