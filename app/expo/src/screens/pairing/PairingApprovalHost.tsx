import { useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';

import { parseEngineError } from '@/engine/nativeModule';
import { useEngineStore } from '@/engine/store';
import { useSendSheetStore } from '@/screens/sendSheetStore';
import { useMessageStore } from '@/stores/messageStore';
import { PairingApprovalSheet } from './PairingApprovalSheet';
import type { ApprovalSheetPhase } from './PairingApprovalSheet.types';
import { usePairingUIStore } from './pairingStore';
import {
  deviceApprovalKey,
  deviceDisplayName,
  isDeviceConnected,
  needsDeviceApproval,
} from './pairingUtils';

/**
 * Global pairing approval host mounted alongside `SendSheetHost` in `AppNavigator`.
 *
 * Whenever a pending pairing request arrives (whether the user is on Clipboard,
 * Devices, or Settings), this host proactively pops up the fingerprint verification
 * sheet so the user never has to hunt for a hidden row.
 */
export function PairingApprovalHost() {
  const { t } = useTranslation();
  const mode = useEngineStore((s) => s.mode);
  const bootStage = useEngineStore((s) => s.bootStage);
  const devices = useEngineStore((s) => s.devices);
  const sendQueueLen = useSendSheetStore((s) => s.queue.length);

  const inlinePairingActive = usePairingUIStore((s) => s.inlinePairingActive);
  const dismissedApprovalKeys = usePairingUIStore((s) => s.dismissedApprovalKeys);
  const manualApprovalDeviceId = usePairingUIStore((s) => s.manualApprovalDeviceId);

  const [busy, setBusy] = useState(false);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [postApproval, setPostApproval] = useState<{
    id: string;
    name: string;
    phase: 'waiting-remote' | 'connected';
  } | null>(null);

  const activeInMain = mode === 'live' && bootStage === 'main';

  const manualDevice =
    activeInMain && manualApprovalDeviceId
      ? (devices.find((d) => d.id === manualApprovalDeviceId && needsDeviceApproval(d)) ?? null)
      : null;

  const autoDevice =
    activeInMain && !inlinePairingActive && sendQueueLen === 0 && !postApproval
      ? (devices.find(
          (d) => needsDeviceApproval(d) && !dismissedApprovalKeys[deviceApprovalKey(d)],
        ) ?? null)
      : null;

  const verifyDevice = manualDevice ?? autoDevice;

  // Clear stale manual target if it was resolved or disappeared.
  useEffect(() => {
    if (manualApprovalDeviceId && !manualDevice) {
      usePairingUIStore.getState().closeManualApproval();
    }
  }, [manualApprovalDeviceId, manualDevice]);

  // Reset inline error when switching target device.
  useEffect(() => {
    setErrorMessage(null);
  }, [verifyDevice?.id, verifyDevice?.revision]);

  // Watch post-approval peer: transition from waiting-remote -> connected live.
  useEffect(() => {
    if (!postApproval || postApproval.phase === 'connected') return;
    const peer = devices.find((d) => d.id === postApproval.id);
    if (peer && isDeviceConnected(peer)) {
      setPostApproval({
        id: peer.id,
        name: deviceDisplayName(peer) || postApproval.name,
        phase: 'connected',
      });
    }
  }, [postApproval, devices]);

  // If the inline pairing sheet opens while a post-approval state was lingering, clear it.
  useEffect(() => {
    if (inlinePairingActive && postApproval) {
      setPostApproval(null);
    }
  }, [inlinePairingActive, postApproval]);

  const errorText = useCallback(
    (err: unknown) => {
      const code = parseEngineError(err).code;
      return t(`errors:engine.${code}`, { defaultValue: t('errors:engine.internal') });
    },
    [t],
  );

  const handleApprove = useCallback(async () => {
    if (!verifyDevice || busy) return;
    setBusy(true);
    setErrorMessage(null);
    const targetId = verifyDevice.id;
    const targetName = deviceDisplayName(verifyDevice);
    try {
      await useEngineStore.getState().approveDevice(verifyDevice, verifyDevice.fingerprint);
      usePairingUIStore.getState().closeManualApproval();
      const updated = useEngineStore.getState().devices.find((d) => d.id === targetId);
      setPostApproval({
        id: targetId,
        name: deviceDisplayName(updated) || targetName,
        phase: isDeviceConnected(updated) ? 'connected' : 'waiting-remote',
      });
    } catch (err) {
      setErrorMessage(errorText(err));
    } finally {
      setBusy(false);
    }
  }, [verifyDevice, busy, errorText]);

  const handleReject = useCallback(async () => {
    if (!verifyDevice || busy || verifyDevice.trust !== 'pending') return;
    setBusy(true);
    setErrorMessage(null);
    try {
      await useEngineStore.getState().unpairDevice(verifyDevice.id, verifyDevice.revision);
      usePairingUIStore.getState().closeManualApproval();
      useMessageStore.getState().showMessage(t('settingsSync:pairing.rejectedNotice'), 'info');
    } catch (err) {
      setErrorMessage(errorText(err));
    } finally {
      setBusy(false);
    }
  }, [verifyDevice, busy, errorText, t]);

  const handleDismiss = useCallback(() => {
    if (busy) return;
    if (postApproval) {
      setPostApproval(null);
      return;
    }
    if (verifyDevice) {
      usePairingUIStore.getState().dismissApprovalKey(deviceApprovalKey(verifyDevice));
    } else {
      usePairingUIStore.getState().closeManualApproval();
    }
  }, [busy, postApproval, verifyDevice]);

  if (!activeInMain) return null;

  const visible = Boolean(postApproval || verifyDevice);
  const phase: ApprovalSheetPhase = postApproval ? postApproval.phase : 'verify';
  const peerName = postApproval?.name ?? deviceDisplayName(verifyDevice);

  return (
    <PairingApprovalSheet
      visible={visible}
      phase={phase}
      device={postApproval ? null : verifyDevice}
      peerName={peerName}
      busy={busy}
      errorMessage={errorMessage}
      onApprove={() => void handleApprove()}
      onReject={() => void handleReject()}
      onDismiss={handleDismiss}
    />
  );
}
