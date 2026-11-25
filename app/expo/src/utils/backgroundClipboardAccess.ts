import type {
  BackgroundSyncStatus,
  ClipboardAccessMethod,
  ClipboardAuthorizationState,
} from '@/engine/types';
import { unicEngineNative } from '@/engine/nativeModule';

/** Shown in the ADB guide so users can verify the phone is listed as `device`. */
export const ADB_DEVICES_COMMAND = 'adb devices';

/** Starts the Shizuku service from a computer (documented by Shizuku). */
export const SHIZUKU_START_COMMAND =
  'adb shell sh /storage/emulated/0/Android/data/moe.shizuku.privileged.api/start.sh';

export type BackgroundClipboardSetupIssue =
  | 'service-unavailable'
  | 'permission-required'
  | 'system-restriction'
  | 'monitoring-setup-required';

export interface BackgroundClipboardSetupState {
  status: 'ready' | 'action-required';
  issue: BackgroundClipboardSetupIssue | null;
}

export type BackgroundClipboardSetupActionResult =
  | 'completed'
  | 'waiting-for-return'
  | 'command-copied'
  | 'no-action'
  | 'failed';

export function getBackgroundClipboardSetupState(
  state: ClipboardAuthorizationState
): BackgroundClipboardSetupState {
  if (state.status === 'unavailable') {
    return { status: 'action-required', issue: 'service-unavailable' };
  }
  if (state.status === 'unauthorized') {
    return { status: 'action-required', issue: 'permission-required' };
  }
  if (state.status === 'incompatible') {
    return { status: 'action-required', issue: 'system-restriction' };
  }
  if (state.monitoring_status === 'setup-required') {
    return { status: 'action-required', issue: 'monitoring-setup-required' };
  }
  return { status: 'ready', issue: null };
}

export function getAdbGrantCommand(status: BackgroundSyncStatus): string {
  return (
    status.authorization.setup_command ??
    `adb shell pm grant ${status.application_id} android.permission.READ_LOGS`
  );
}

/**
 * Executes the next native setup action required for the currently selected Android background
 * clipboard reading method.
 */
export async function continueBackgroundClipboardSetup(
  method: ClipboardAccessMethod,
  state: ClipboardAuthorizationState
): Promise<BackgroundClipboardSetupActionResult> {
  if (method === 'shizuku') {
    if (state.status === 'incompatible') {
      const ok = await unicEngineNative.resolveBackgroundClipboardRestriction();
      return ok ? 'completed' : 'failed';
    }
    if (state.status === 'unavailable') {
      const opened = await unicEngineNative.openShizukuAppOrGuide();
      return opened ? 'waiting-for-return' : 'failed';
    }
    if (state.status === 'unauthorized') {
      const requested = await unicEngineNative.requestShizukuPermission();
      return requested ? 'waiting-for-return' : 'failed';
    }
    return 'no-action';
  }

  if (state.status === 'unauthorized') {
    const requested = await unicEngineNative.requestOverlayPermission();
    return requested ? 'waiting-for-return' : 'failed';
  }
  if (method === 'overlay-event' && state.setup_command) {
    await unicEngineNative.copyPlainText(state.setup_command);
    return 'command-copied';
  }
  return 'no-action';
}
