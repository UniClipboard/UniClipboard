import type { ReactNode } from 'react';

export interface OnboardingScannerController {
  openScanner: () => void;
  renderScannerOverlay: () => ReactNode;
}
