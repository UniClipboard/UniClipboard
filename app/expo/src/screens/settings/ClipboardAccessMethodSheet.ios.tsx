import type { ReactNode } from 'react';
import type { ClipboardAccessMethodSheetController } from './ClipboardAccessMethodSheet.types';

const NOOP_CONTROLLER: ClipboardAccessMethodSheetController = {
  openMethodSheet: () => {},
  openAdbSetupSheet: () => {},
  openShizukuSetupSheet: () => {},
  openRestrictionSheet: () => {},
  closeSheet: () => {},
};

export function useClipboardAccessMethodSheet(): ClipboardAccessMethodSheetController {
  return NOOP_CONTROLLER;
}

export function ClipboardAccessMethodSheetProvider({ children }: { children: ReactNode }) {
  return children;
}
