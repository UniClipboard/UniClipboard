import type { ClipboardAccessMethod } from '@/engine/types';

export interface OpenClipboardAccessMethodSheetOptions {
  selectedMethod: ClipboardAccessMethod;
  onSelect: (method: ClipboardAccessMethod) => void | Promise<void>;
}

export type AdbSetupStage = 'instructions' | 'copied' | 'notDetected';

export interface OpenAdbSetupSheetOptions {
  stage: AdbSetupStage;
  /** The one-time `pm grant` command; copying it is owned by the caller. */
  command: string;
  onCopy: () => void;
  onCheck: () => void;
}

export type ShizukuSetupStage = 'notRunning' | 'authorize';

export interface OpenShizukuSetupSheetOptions {
  stage: ShizukuSetupStage;
  /** Optional computer-side command that starts the Shizuku service. */
  command: string;
  /** Primary action: open Shizuku (notRunning) or request permission (authorize). */
  onAction: () => void;
  onCheck: () => void;
}

export interface OpenClipboardRestrictionSheetOptions {
  onConfirm: () => void;
}

export interface ClipboardAccessMethodSheetController {
  openMethodSheet: (options: OpenClipboardAccessMethodSheetOptions) => void;
  openAdbSetupSheet: (options: OpenAdbSetupSheetOptions) => void;
  openShizukuSetupSheet: (options: OpenShizukuSetupSheetOptions) => void;
  openRestrictionSheet: (options: OpenClipboardRestrictionSheetOptions) => void;
  closeSheet: () => void;
}
