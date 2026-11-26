import { create } from 'zustand';

interface PairingUIState {
  /**
   * True while the Devices screen's inline pairing flow sheet is open.
   * When true, the global PairingApprovalHost defers to the inline flow so
   * two native sheets never collide.
   */
  inlinePairingActive: boolean;
  /**
   * Approval snapshot keys the user explicitly dismissed ("Later").
   * Prevents nagging for the exact same pending snapshot while still popping up
   * immediately if a new device or new revision arrives.
   */
  dismissedApprovalKeys: Record<string, true>;
  /**
   * Device ID explicitly tapped by the user to review from the pending list.
   */
  manualApprovalDeviceId: string | null;

  setInlinePairingActive: (active: boolean) => void;
  dismissApprovalKey: (key: string) => void;
  openManualApproval: (deviceId: string) => void;
  closeManualApproval: () => void;
}

export const usePairingUIStore = create<PairingUIState>((set) => ({
  inlinePairingActive: false,
  dismissedApprovalKeys: {},
  manualApprovalDeviceId: null,

  setInlinePairingActive: (inlinePairingActive) => set({ inlinePairingActive }),

  dismissApprovalKey: (key) =>
    set((state) => ({
      dismissedApprovalKeys: { ...state.dismissedApprovalKeys, [key]: true },
      manualApprovalDeviceId: null,
    })),

  openManualApproval: (deviceId) => set({ manualApprovalDeviceId: deviceId }),

  closeManualApproval: () => set({ manualApprovalDeviceId: null }),
}));
