import { useCallback, useEffect, useRef, useState } from 'react';
import { AppState } from 'react-native';
import { useTranslation } from 'react-i18next';
import { parseEngineError, unicEngineNative } from '@/engine/nativeModule';
import { useEngineStore } from '@/engine/store';
import type {
  EngineCandidate,
  EngineDevice,
  EngineEvent,
  EngineOperation,
  SensitiveTicket,
} from '@/engine/types';
import { useMessageStore } from '@/stores/messageStore';
import { usePairingUIStore } from './pairingStore';
import {
  classifyPairingInput,
  deviceApprovalKey,
  deviceDisplayName,
  formatCountdown,
  isDeviceConnected,
  needsDeviceApproval,
} from './pairingUtils';

export type PairingFlowTab = 'enter' | 'show';
export type PairingFlowStage =
  | 'idle'
  | 'connecting'
  | 'verify'
  | 'waiting-remote'
  | 'connected'
  | 'error';

export function usePairingFlow(focused: boolean) {
  const { t } = useTranslation();
  const mode = useEngineStore((s) => s.mode);
  const devices = useEngineStore((s) => s.devices);
  const pairingCode = useEngineStore((s) => s.pairingCode);

  const [open, setOpen] = useState(false);
  const [tab, setTab] = useState<PairingFlowTab>('enter');
  const [stage, setStage] = useState<PairingFlowStage>('idle');
  const [busy, setBusy] = useState(false);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);

  const [creatingCode, setCreatingCode] = useState(false);
  const [codeError, setCodeError] = useState<string | null>(null);
  const [countdown, setCountdown] = useState('');

  const [ticket, setTicket] = useState<SensitiveTicket | null>(null);
  const [exportingTicket, setExportingTicket] = useState(false);
  const [foreground, setForeground] = useState(AppState.currentState === 'active');

  const [operationID, setOperationID] = useState<string | null>(null);
  const [operation, setOperation] = useState<EngineOperation | null>(null);
  const [verifyDeviceId, setVerifyDeviceId] = useState<string | null>(null);
  const [approvedPeer, setApprovedPeer] = useState<{ id: string; name: string } | null>(null);

  const startingRef = useRef(false);
  const opGeneration = useRef(0);
  const ticketGeneration = useRef(0);
  const exportingRef = useRef(false);
  const creatingCodeRef = useRef(false);
  const baselinePendingKeys = useRef<Set<string>>(new Set());

  const errorText = useCallback(
    (err: unknown) => {
      const code = parseEngineError(err).code;
      return t(`errors:engine.${code}`, { defaultValue: t('errors:engine.internal') });
    },
    [t],
  );

  const captureBaselinePending = useCallback(() => {
    baselinePendingKeys.current = new Set(
      useEngineStore
        .getState()
        .devices.filter(needsDeviceApproval)
        .map(deviceApprovalKey),
    );
  }, []);

  const clearTicket = useCallback(() => {
    ticketGeneration.current++;
    setTicket(null);
  }, []);

  // Inform global PairingApprovalHost while this inline sheet owns the interaction.
  useEffect(() => {
    usePairingUIStore.getState().setInlinePairingActive(open);
    return () => {
      usePairingUIStore.getState().setInlinePairingActive(false);
    };
  }, [open]);

  // Clear sensitive offline QR ticket whenever app backgrounds or screen loses focus.
  useEffect(() => {
    const sub = AppState.addEventListener('change', (state) => {
      const active = state === 'active';
      setForeground(active);
      if (!active) clearTicket();
    });
    return () => sub.remove();
  }, [clearTicket]);

  useEffect(() => {
    if (!focused || mode !== 'live') {
      clearTicket();
    }
  }, [focused, mode, clearTicket]);

  useEffect(() => {
    if (!ticket) return;
    const remain = Math.max(0, ticket.expires_at_ms - Date.now());
    const timer = setTimeout(clearTicket, remain);
    return () => clearTimeout(timer);
  }, [ticket, clearTicket]);

  // Live countdown for active 6-digit pairing code.
  useEffect(() => {
    if (!pairingCode) {
      setCountdown('');
      return;
    }
    const update = () => {
      const now = Date.now();
      if (pairingCode.expires_at_ms <= now) {
        setCountdown('');
        void useEngineStore.getState().refreshDevicesAndCandidates();
        return;
      }
      setCountdown(formatCountdown(pairingCode.expires_at_ms, now));
    };
    update();
    const timer = setInterval(update, 1000);
    return () => clearInterval(timer);
  }, [pairingCode]);

  const generateCode = useCallback(async () => {
    if (creatingCodeRef.current || mode !== 'live') return;
    creatingCodeRef.current = true;
    setCreatingCode(true);
    setCodeError(null);
    try {
      await useEngineStore.getState().createPairingCode();
    } catch (err) {
      setCodeError(errorText(err));
    } finally {
      creatingCodeRef.current = false;
      setCreatingCode(false);
    }
  }, [mode, errorText]);

  // Auto-generate pairing code when user opens the "This phone's code" tab.
  useEffect(() => {
    if (open && stage === 'idle' && tab === 'show' && !pairingCode && !codeError && mode === 'live') {
      void generateCode();
    }
  }, [open, stage, tab, pairingCode, codeError, mode, generateCode]);

  // Watch active join/pair operation.
  useEffect(() => {
    if (!operationID || mode !== 'live') return;
    let reqSeq = 0;
    let disposed = false;

    const refresh = async () => {
      const current = ++reqSeq;
      try {
        const result = await unicEngineNative.getOperation(operationID);
        if (disposed || current !== reqSeq) return;
        setOperation(result);
        if (result.error?.code) {
          setOperationID(null);
          setErrorMessage(
            t(`errors:engine.${result.error.code}`, {
              defaultValue: t('errors:engine.internal'),
            }),
          );
          setStage('error');
          return;
        }
        if (result.pairing?.peer_id) {
          await useEngineStore.getState().refreshDevicesAndCandidates();
        }
      } catch (err) {
        if (disposed || current !== reqSeq) return;
        setOperationID(null);
        setErrorMessage(errorText(err));
        setStage('error');
      }
    };

    const events = unicEngineNative.addEventListener((raw) => {
      let event: EngineEvent;
      try {
        event = JSON.parse(raw) as EngineEvent;
      } catch {
        return;
      }
      if (event.kind === 'operations' && event.objects?.some(({ id }) => id === operationID)) {
        void refresh();
      }
    });
    const resync = unicEngineNative.addResyncListener(() => void refresh());
    const active = AppState.addEventListener('change', (state) => {
      if (state === 'active') void refresh();
    });
    void refresh();

    return () => {
      disposed = true;
      events.remove();
      resync.remove();
      active.remove();
    };
  }, [operationID, mode, errorText, t]);

  // Auto-advance stages as Engine devices update.
  useEffect(() => {
    if (!open || mode !== 'live') return;

    // 1. Connecting -> inline verify (or connected) once peer appears.
    if (stage === 'connecting') {
      const peerId = operation?.pairing?.peer_id;
      if (peerId) {
        const peer = devices.find((d) => d.id === peerId);
        if (peer) {
          if (needsDeviceApproval(peer)) {
            clearTicket();
            setVerifyDeviceId(peer.id);
            setStage('verify');
            return;
          }
          if (peer.trust === 'trusted' && isDeviceConnected(peer)) {
            setApprovedPeer({ id: peer.id, name: deviceDisplayName(peer) });
            setOperationID(null);
            setStage('connected');
            return;
          }
        }
      }
    }

    // 2. Showing own code/QR -> immediately clear secret & jump to inline verify when peer joins.
    if (stage === 'idle' && tab === 'show') {
      const incoming = devices.find(
        (d) => needsDeviceApproval(d) && !baselinePendingKeys.current.has(deviceApprovalKey(d)),
      );
      if (incoming) {
        clearTicket();
        setVerifyDeviceId(incoming.id);
        setStage('verify');
        return;
      }
    }

    // 3. Waiting for remote approval -> transition to connected as soon as binding connects.
    if (stage === 'waiting-remote' && approvedPeer) {
      const peer = devices.find((d) => d.id === approvedPeer.id);
      if (peer && isDeviceConnected(peer)) {
        setApprovedPeer({ id: peer.id, name: deviceDisplayName(peer) });
        setStage('connected');
      }
    }
  }, [open, mode, stage, tab, devices, operation, approvedPeer, clearTicket]);

  const resetTransient = useCallback(() => {
    opGeneration.current++;
    clearTicket();
    setOperationID(null);
    setOperation(null);
    setVerifyDeviceId(null);
    setApprovedPeer(null);
    setErrorMessage(null);
    setCodeError(null);
    setBusy(false);
    setStage('idle');
  }, [clearTicket]);

  const openCodeSheet = useCallback(
    (initialTab: PairingFlowTab = 'enter') => {
      captureBaselinePending();
      resetTransient();
      setTab(initialTab);
      setOpen(true);
    },
    [captureBaselinePending, resetTransient],
  );

  const selectTab = useCallback(
    (nextTab: PairingFlowTab) => {
      if (nextTab !== 'show') {
        clearTicket();
      } else {
        setCodeError(null);
      }
      setErrorMessage(null);
      setTab(nextTab);
    },
    [clearTicket],
  );

  const close = useCallback(() => {
    if (stage === 'verify' && verifyDeviceId) {
      const device = useEngineStore.getState().devices.find((d) => d.id === verifyDeviceId);
      if (device && needsDeviceApproval(device)) {
        usePairingUIStore.getState().dismissApprovalKey(deviceApprovalKey(device));
      }
    }
    setOpen(false);
    resetTransient();
  }, [stage, verifyDeviceId, resetTransient]);

  const startOperation = useCallback(
    async (action: () => Promise<string>) => {
      if (startingRef.current) return;
      startingRef.current = true;
      captureBaselinePending();
      clearTicket();
      const generation = ++opGeneration.current;
      setErrorMessage(null);
      setOperation(null);
      setOperationID(null);
      setVerifyDeviceId(null);
      setApprovedPeer(null);
      setStage('connecting');
      setOpen(true);
      try {
        const id = await action();
        if (generation === opGeneration.current) {
          setOperationID(id);
        }
      } catch (err) {
        if (generation === opGeneration.current) {
          setErrorMessage(errorText(err));
          setStage('error');
        }
      } finally {
        startingRef.current = false;
      }
    },
    [captureBaselinePending, clearTicket, errorText],
  );

  const submitInput = useCallback(
    (rawInput: string) => {
      const classified = classifyPairingInput(rawInput);
      const store = useEngineStore.getState();
      if (classified.kind === 'code') {
        void startOperation(() => store.joinByCode(classified.normalized));
      } else if (classified.kind === 'ticket') {
        void startOperation(() => store.joinByTicket(classified.normalized));
      } else {
        setErrorMessage(t('settingsSync:pairing.invalidInput'));
        if (stage !== 'idle') {
          setStage('error');
        }
      }
    },
    [startOperation, stage, t],
  );

  const pairCandidate = useCallback(
    (candidate: EngineCandidate) => {
      const store = useEngineStore.getState();
      void startOperation(() => store.pairCandidate(candidate));
    },
    [startOperation],
  );

  const exportOfflineTicket = useCallback(async () => {
    if (exportingRef.current || mode !== 'live') return;
    exportingRef.current = true;
    setExportingTicket(true);
    setCodeError(null);
    const generation = ++ticketGeneration.current;
    try {
      const result = await useEngineStore.getState().exportPairingTicket();
      if (generation === ticketGeneration.current && result.expires_at_ms > Date.now()) {
        setTicket(result);
      }
    } catch (err) {
      if (generation === ticketGeneration.current) {
        setCodeError(errorText(err));
      }
    } finally {
      exportingRef.current = false;
      setExportingTicket(false);
    }
  }, [mode, errorText]);

  const copyPairingCode = useCallback(async () => {
    if (!pairingCode?.code) return;
    try {
      await unicEngineNative.copyPlainText(pairingCode.code);
      useMessageStore.getState().showMessage(t('errors:copy.copied'), 'success');
    } catch (err) {
      setCodeError(errorText(err));
    }
  }, [pairingCode, t, errorText]);

  const copyOfflineTicket = useCallback(async () => {
    if (!ticket) return;
    if (ticket.expires_at_ms <= Date.now()) {
      clearTicket();
      return;
    }
    try {
      await unicEngineNative.copyPairingTicket(ticket.ticket, ticket.expires_at_ms);
      useMessageStore.getState().showMessage(t('errors:copy.copied'), 'success');
    } catch (err) {
      setCodeError(errorText(err));
    }
  }, [ticket, clearTicket, t, errorText]);

  const cancelCode = useCallback(async () => {
    try {
      await useEngineStore.getState().cancelPairingCode();
    } catch (err) {
      setCodeError(errorText(err));
    }
  }, [errorText]);

  const verifyDevice: EngineDevice | null = verifyDeviceId
    ? (devices.find((d) => d.id === verifyDeviceId) ?? null)
    : null;

  const approveVerifyDevice = useCallback(async () => {
    if (!verifyDevice || busy) return;
    setBusy(true);
    setErrorMessage(null);
    const targetName = deviceDisplayName(verifyDevice);
    const targetId = verifyDevice.id;
    try {
      await useEngineStore.getState().approveDevice(verifyDevice, verifyDevice.fingerprint);
      const updated = useEngineStore.getState().devices.find((d) => d.id === targetId);
      setApprovedPeer({ id: targetId, name: deviceDisplayName(updated) || targetName });
      setOperationID(null);
      setVerifyDeviceId(null);
      if (isDeviceConnected(updated)) {
        setStage('connected');
      } else {
        setStage('waiting-remote');
      }
    } catch (err) {
      setErrorMessage(errorText(err));
    } finally {
      setBusy(false);
    }
  }, [verifyDevice, busy, errorText]);

  const rejectVerifyDevice = useCallback(async () => {
    if (!verifyDevice || busy || verifyDevice.trust !== 'pending') return;
    setBusy(true);
    setErrorMessage(null);
    try {
      await useEngineStore.getState().unpairDevice(verifyDevice.id, verifyDevice.revision);
      useMessageStore.getState().showMessage(t('settingsSync:pairing.rejectedNotice'), 'info');
      setOpen(false);
      resetTransient();
    } catch (err) {
      setErrorMessage(errorText(err));
      setBusy(false);
    }
  }, [verifyDevice, busy, errorText, resetTransient, t]);

  const retryEntry = useCallback(() => {
    opGeneration.current++;
    setOperationID(null);
    setOperation(null);
    setErrorMessage(null);
    setTab('enter');
    setStage('idle');
  }, []);

  const handshakeStageIndex = (() => {
    const phase = operation?.phase ?? 'resolving';
    if (phase === 'connecting') return 1;
    if (phase === 'handshaking' || phase === 'completed' || phase === 'succeeded') return 2;
    return 0;
  })();

  return {
    open,
    tab,
    stage,
    busy,
    errorMessage,
    creatingCode,
    codeError,
    pairingCode,
    countdown,
    ticket: focused && foreground ? ticket : null,
    exportingTicket,
    verifyDevice,
    approvedPeer,
    handshakeStageIndex,

    openCodeSheet,
    selectTab,
    close,
    submitInput,
    pairCandidate,
    generateCode,
    cancelCode,
    copyPairingCode,
    exportOfflineTicket,
    clearTicket,
    copyOfflineTicket,
    approveVerifyDevice,
    rejectVerifyDevice,
    retryEntry,
  };
}
