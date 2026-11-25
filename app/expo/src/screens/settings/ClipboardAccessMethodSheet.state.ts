import type { ClipboardAuthorizationState } from '@/engine/types';
import { getBackgroundClipboardSetupState } from '@/utils/backgroundClipboardAccess';
import type {
  OpenAdbSetupSheetOptions,
  OpenClipboardAccessMethodSheetOptions,
  OpenClipboardRestrictionSheetOptions,
  OpenShizukuSetupSheetOptions,
} from './ClipboardAccessMethodSheet.types';

export type ClipboardAccessSheetContent =
  | ({ type: 'methods' } & OpenClipboardAccessMethodSheetOptions)
  | ({ type: 'adb' } & OpenAdbSetupSheetOptions)
  | ({ type: 'shizuku' } & OpenShizukuSetupSheetOptions)
  | ({ type: 'restriction' } & OpenClipboardRestrictionSheetOptions);

export interface ClipboardAccessSheetState {
  visible: boolean;
  content: ClipboardAccessSheetContent | null;
  isSelecting: boolean;
}

export const INITIAL_CLIPBOARD_ACCESS_SHEET_STATE: ClipboardAccessSheetState = {
  visible: false,
  content: null,
  isSelecting: false,
};

export type ClipboardAccessSheetAction =
  | { type: 'open-methods'; options: OpenClipboardAccessMethodSheetOptions }
  | { type: 'open-adb'; options: OpenAdbSetupSheetOptions }
  | { type: 'open-shizuku'; options: OpenShizukuSetupSheetOptions }
  | { type: 'open-restriction'; options: OpenClipboardRestrictionSheetOptions }
  | { type: 'close' }
  | { type: 'selection-started' }
  | { type: 'selection-finished' };

export function clipboardAccessSheetReducer(
  state: ClipboardAccessSheetState,
  action: ClipboardAccessSheetAction
): ClipboardAccessSheetState {
  switch (action.type) {
    case 'open-methods':
      return {
        visible: true,
        content: { type: 'methods', ...action.options },
        isSelecting: false,
      };
    case 'open-adb':
      return {
        visible: true,
        content: { type: 'adb', ...action.options },
        isSelecting: false,
      };
    case 'open-shizuku':
      return {
        visible: true,
        content: { type: 'shizuku', ...action.options },
        isSelecting: false,
      };
    case 'open-restriction':
      return {
        visible: true,
        content: { type: 'restriction', ...action.options },
        isSelecting: false,
      };
    case 'close':
      return { ...state, visible: false };
    case 'selection-started':
      return { ...state, visible: false, isSelecting: true };
    case 'selection-finished':
      return { ...state, isSelecting: false };
  }
}

const SHEET_MAX_HEIGHT_RATIO = 0.9;
const SHEET_HANDLE_HEIGHT = 28;

export function getSheetContentMaxHeight(windowHeight: number): number {
  return windowHeight * SHEET_MAX_HEIGHT_RATIO - SHEET_HANDLE_HEIGHT;
}

export type AdbAuthorizationCheckOutcome = 'complete' | 'show-adb-guide' | 'continue-access-setup';

export function resolveAdbAuthorizationCheck(
  state: ClipboardAuthorizationState
): AdbAuthorizationCheckOutcome {
  const setupState = getBackgroundClipboardSetupState(state);
  if (setupState.status === 'ready') return 'complete';
  return setupState.issue === 'monitoring-setup-required'
    ? 'show-adb-guide'
    : 'continue-access-setup';
}
