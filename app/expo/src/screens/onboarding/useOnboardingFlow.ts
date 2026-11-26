import { useCallback, useEffect, useRef, useState } from 'react';
import { AppState } from 'react-native';
import { useTranslation } from 'react-i18next';
import { parseEngineError, unicEngineNative } from '@/engine/nativeModule';
import { useEngineStore } from '@/engine/store';
import type { EngineDevice, EngineEvent, EngineOperation } from '@/engine/types';
import { showUnavailable } from '@/shell/notice';

export type OnboardingStep =
  | 'intro'
  | 'pair'
  | 'pair-code'
  | 'pair-handshake'
  | 'pair-approve'
  | 'pair-success'
  | 'capabilities'
  | 'ready';

export function formatFingerprintLines(value: string): string {
  const groups = value.trim().split(/\s+/).filter(Boolean);
  if (groups.length === 8) {
    return `${groups.slice(0, 4).join(' ')}\n${groups.slice(4).join(' ')}`;
  }
  return value;
}

export function formatPairingCodeDisplay(code: string): string {
  const clean = code.replace(/\D/g, '');
  if (clean.length === 6) {
    return `${clean.slice(0, 3)} ${clean.slice(3)}`;
  }
  return code;
}

export function deviceDisplayName(device: EngineDevice | null): string {
  if (!device) return '';
  return device.name || device.reported_name || device.id.slice(0, 8);
}

export function useOnboardingFlow(onComplete: () => void | Promise<void>) {
  const { t } = useTranslation();
  const mode = useEngineStore((s) => s.mode);
  const devices = useEngineStore((s) => s.devices);
  const pairingCode = useEngineStore((s) => s.pairingCode);

  const [step, setStep] = useState<OnboardingStep>('intro');
  const [slide, setSlide] = useState(0);
  const [busy, setBusy] = useState(false);
  const [finishing, setFinishing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Pairing flow state
  const [codeDigits, setCodeDigits] = useState<string[]>(['', '', '', '', '', '']);
  const [ticketMode, setTicketMode] = useState(false);
  const [ticketInput, setTicketInput] = useState('');
  const [showMyCode, setShowMyCode] = useState(false);
  const [myCodeCountdown, setMyCodeCountdown] = useState('');
  const [myCodeCopied, setMyCodeCopied] = useState(false);

  const [operationID, setOperationID] = useState<string | null>(null);
  const [operation, setOperation] = useState<EngineOperation | null>(null);
  const [pendingDevice, setPendingDevice] = useState<EngineDevice | null>(null);
  const [pairedDevice, setPairedDevice] = useState<EngineDevice | null>(null);

  const completingRef = useRef(false);
  const startingOpRef = useRef(false);
  const opGeneration = useRef(0);
  const knownPendingIds = useRef<Set<string>>(new Set());

  const errorText = useCallback(
    (err: unknown) => {
      const code = parseEngineError(err).code;
      return t(`errors:engine.${code}`, { defaultValue: t('errors:engine.internal') });
    },
    [t],
  );

  // Keep countdown ticker for generated pairing code.
  useEffect(() => {
    if (!showMyCode || !pairingCode) {
      setMyCodeCountdown('');
      return;
    }
    const update = () => {
      const remainMs = Math.max(0, pairingCode.expires_at_ms - Date.now());
      const totalSec = Math.ceil(remainMs / 1000);
      const mins = Math.floor(totalSec / 60);
      const secs = totalSec % 60;
      setMyCodeCountdown(`${String(mins).padStart(2, '0')}:${String(secs).padStart(2, '0')}`);
    };
    update();
    const timer = setInterval(update, 1000);
    return () => clearInterval(timer);
  }, [showMyCode, pairingCode]);

  // Watch active pairing operation (resolving -> connecting -> handshaking -> approval / error).
  useEffect(() => {
    if (!operationID || mode !== 'live') return;
    let request = 0;
    let disposed = false;

    const refresh = async () => {
      const current = ++request;
      try {
        const result = await unicEngineNative.getOperation(operationID);
        if (disposed || current !== request) return;
        setOperation(result);
        if (result.error?.code) {
          setOperationID(null);
          setError(
            t(`errors:engine.${result.error.code}`, {
              defaultValue: t('errors:engine.internal'),
            }),
          );
          setStep('pair-code');
          return;
        }
        if (result.pairing?.peer_id) {
          await useEngineStore.getState().refreshDevicesAndCandidates();
        }
      } catch (err) {
        if (disposed || current !== request) return;
        setOperationID(null);
        setError(errorText(err));
        setStep('pair-code');
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

  // React to Engine device transitions: pending approval -> pair-approve, trusted -> pair-success.
  useEffect(() => {
    if (mode !== 'live') return;

    const peerId = operation?.pairing?.peer_id;
    if (peerId) {
      const peer = devices.find((d) => d.id === peerId);
      if (peer) {
        if (peer.trust === 'pending' && step === 'pair-handshake') {
          setPendingDevice(peer);
          setStep('pair-approve');
          return;
        }
        if (peer.trust === 'trusted' && (step === 'pair-handshake' || step === 'pair-approve')) {
          setPairedDevice(peer);
          setPendingDevice(null);
          setOperationID(null);
          setStep('pair-success');
          return;
        }
      }
    }

    // If user generated a pairing code on this phone and a remote peer joined, surface approval.
    if (showMyCode && step === 'pair') {
      const incoming = devices.find(
        (d) => d.trust === 'pending' && !knownPendingIds.current.has(d.id),
      );
      if (incoming) {
        setPendingDevice(incoming);
        setShowMyCode(false);
        setStep('pair-approve');
      }
    }
  }, [devices, operation, mode, step, showMyCode]);

  const finishOnboarding = useCallback(async () => {
    if (completingRef.current) return;
    completingRef.current = true;
    setFinishing(true);
    setError(null);
    try {
      await onComplete();
    } catch {
      setError(t('onboarding:intro.saveError'));
    } finally {
      completingRef.current = false;
      setFinishing(false);
    }
  }, [onComplete, t]);

  const beginSetup = useCallback(async () => {
    setError(null);
    if (mode === 'live') {
      setBusy(true);
      try {
        await useEngineStore.getState().ensureEngineStartedForOnboarding();
        knownPendingIds.current = new Set(
          useEngineStore
            .getState()
            .devices.filter((d) => d.trust === 'pending')
            .map((d) => d.id),
        );
      } catch (err) {
        setError(errorText(err));
        setBusy(false);
        return;
      } finally {
        setBusy(false);
      }
    }
    setStep('pair');
  }, [mode, errorText]);

  const startPairingOperation = useCallback(
    async (action: () => Promise<string>) => {
      if (startingOpRef.current) return;
      startingOpRef.current = true;
      const gen = ++opGeneration.current;
      setError(null);
      setOperation(null);
      setOperationID(null);
      setStep('pair-handshake');
      try {
        const id = await action();
        if (gen === opGeneration.current) {
          setOperationID(id);
        }
      } catch (err) {
        if (gen === opGeneration.current) {
          setError(errorText(err));
          setStep('pair-code');
        }
      } finally {
        startingOpRef.current = false;
      }
    },
    [errorText],
  );

  const submitPairingInput = useCallback(
    (rawInput: string) => {
      const raw = rawInput.trim();
      if (!raw) return;
      if (mode !== 'live') {
        showUnavailable();
        return;
      }
      const digits = raw.replace(/\s+/g, '');
      const store = useEngineStore.getState();
      if (/^\d{3}-?\d{3}$/.test(digits)) {
        void startPairingOperation(() => store.joinByCode(digits));
      } else if (raw.startsWith('unic-pair:')) {
        void startPairingOperation(() => store.joinByTicket(raw));
      } else {
        setError(t('settingsSync:pairing.invalidInput'));
        setStep('pair-code');
      }
    },
    [mode, startPairingOperation, t],
  );

  const submitCodeForm = useCallback(() => {
    if (ticketMode) {
      submitPairingInput(ticketInput);
      return;
    }
    submitPairingInput(codeDigits.join(''));
  }, [ticketMode, ticketInput, codeDigits, submitPairingInput]);

  const toggleMyCode = useCallback(async () => {
    setError(null);
    if (mode !== 'live') {
      showUnavailable();
      return;
    }
    if (showMyCode) {
      setShowMyCode(false);
      void useEngineStore.getState().cancelPairingCode().catch(() => {});
      return;
    }
    setBusy(true);
    try {
      knownPendingIds.current = new Set(
        useEngineStore
          .getState()
          .devices.filter((d) => d.trust === 'pending')
          .map((d) => d.id),
      );
      await useEngineStore.getState().createPairingCode();
      setShowMyCode(true);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  }, [mode, showMyCode, errorText]);

  const copyMyCode = useCallback(async () => {
    if (!pairingCode?.code || mode !== 'live') return;
    try {
      await unicEngineNative.copyPlainText(pairingCode.code);
      setMyCodeCopied(true);
      setTimeout(() => setMyCodeCopied(false), 1800);
    } catch {
      // ignore clipboard write errors
    }
  }, [pairingCode, mode]);

  const cancelHandshake = useCallback(() => {
    opGeneration.current++;
    setOperationID(null);
    setOperation(null);
    setStep('pair');
  }, []);

  const approvePendingDevice = useCallback(async () => {
    if (!pendingDevice || mode !== 'live') return;
    setBusy(true);
    setError(null);
    try {
      await useEngineStore.getState().approveDevice(pendingDevice, pendingDevice.fingerprint);
      setPairedDevice(pendingDevice);
      setPendingDevice(null);
      setOperationID(null);
      setStep('pair-success');
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  }, [pendingDevice, mode, errorText]);

  const rejectPendingDevice = useCallback(async () => {
    const target = pendingDevice;
    setPendingDevice(null);
    setOperationID(null);
    setOperation(null);
    setStep('pair');
    if (target && mode === 'live') {
      knownPendingIds.current.add(target.id);
      try {
        await useEngineStore.getState().unpairDevice(target.id, target.revision);
      } catch {
        // best-effort rejection cleanup
      }
    }
  }, [pendingDevice, mode]);

  const goBack = useCallback(() => {
    setError(null);
    switch (step) {
      case 'pair':
        setStep('intro');
        setSlide(2);
        break;
      case 'pair-code':
      case 'pair-handshake':
      case 'pair-approve':
      case 'pair-success':
        cancelHandshake();
        break;
      case 'capabilities':
        setStep(pairedDevice ? 'pair-success' : 'pair');
        break;
      case 'ready':
        setStep('capabilities');
        break;
      default:
        break;
    }
  }, [step, pairedDevice, cancelHandshake]);

  const handshakeStageIndex = (() => {
    const phase = operation?.phase ?? 'resolving';
    if (phase === 'connecting') return 1;
    if (phase === 'handshaking' || phase === 'completed' || phase === 'succeeded') return 2;
    return 0;
  })();

  const isCodeReady = ticketMode
    ? ticketInput.trim().length > 0
    : codeDigits.every((d) => /^\d$/.test(d));

  return {
    mode,
    step,
    setStep,
    slide,
    setSlide,
    busy,
    finishing,
    error,
    setError,
    codeDigits,
    setCodeDigits,
    ticketMode,
    setTicketMode,
    ticketInput,
    setTicketInput,
    isCodeReady,
    showMyCode,
    pairingCode,
    myCodeCountdown,
    myCodeCopied,
    handshakeStageIndex,
    pendingDevice,
    pairedDevice,
    beginSetup,
    finishOnboarding,
    submitPairingInput,
    submitCodeForm,
    toggleMyCode,
    copyMyCode,
    cancelHandshake,
    approvePendingDevice,
    rejectPendingDevice,
    goBack,
  };
}
