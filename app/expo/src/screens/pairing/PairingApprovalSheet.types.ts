import type { EngineDevice } from '@/engine/types';

export type ApprovalSheetPhase = 'verify' | 'waiting-remote' | 'connected';

export interface PairingApprovalSheetProps {
  visible: boolean;
  phase: ApprovalSheetPhase;
  device: EngineDevice | null;
  peerName: string;
  busy: boolean;
  errorMessage: string | null;
  onApprove: () => void;
  onReject: () => void;
  onDismiss: () => void;
}
