import { useCallback, useEffect, useRef, useState } from 'react';
import { Alert, AppState, Linking, StyleSheet } from 'react-native';
import { useIsFocused, useNavigation } from '@react-navigation/native';
import type { NativeStackNavigationProp } from '@react-navigation/native-stack';
import { Host, NavigationStack, ZStack } from '@expo/ui/swift-ui';
import { frame, tint } from '@expo/ui/swift-ui/modifiers';
import { useTranslation } from 'react-i18next';

import {
  AppAlertDialog,
  IosPageChromeProvider,
  type IosPageChrome,
} from '@/components/ui';
import { parseEngineError, unicEngineNative } from '@/engine/nativeModule';
import { useEngineStore } from '@/engine/store';
import type {
  EngineCandidate,
  EngineDevice,
  EngineEvent,
  EngineOperation,
  SensitiveTicket,
} from '@/engine/types';
import type { RootStackParamList, SpaceDeviceTarget } from '@/navigation/AppNavigator.types';
import { useHideTabBarOnSubPage } from '@/navigation/ios/useHideTabBarOnSubPage';
import { showUnavailable } from '@/shell/notice';
import { useMessageStore } from '@/stores/messageStore';
import { iosAccentColor } from '@/theme/iosDesignTokens';
import { DeviceDetailSheet } from './ios/devices/DeviceDetailSheet';
import { DevicesRootPage } from './ios/devices/DevicesRootPage';
import {
  CandidateDetailSheet,
  PairingJoinSheet,
  PairingTicketSheet,
} from './ios/devices/PairingSheets';
import { deviceDisplayName } from './ios/devices/deviceRows';

const fillModifier = frame({ maxWidth: Infinity, maxHeight: Infinity });
const NAVIGATION_CHROME: IosPageChrome = { kind: 'navigation' };

/**
 * iOS "Devices" tab.
 * Full-screen SwiftUI Host owning the `NavigationStack` and all sibling native sheets.
 */
export function DevicesScreen(_props: SpaceDeviceTarget) {
  const { t } = useTranslation();
  const pair = (key: string, values?: Record<string, string>) =>
    t(`settingsSync:pairing.${key}`, values);
  const navigation = useNavigation<NativeStackNavigationProp<RootStackParamList>>();
  const focused = useIsFocused();

  const [path, setPath] = useState<string[]>([]);
  useHideTabBarOnSubPage(path);

  const mode = useEngineStore((s) => s.mode);
  const self = useEngineStore((s) => s.snapshot?.self);
  const devices = useEngineStore((s) => s.devices);
  const candidates = useEngineStore((s) => s.candidates);
  const pairingCode = useEngineStore((s) => s.pairingCode);

  const [manualJoinOpen, setManualJoinOpen] = useState(false);
  const scanningRef = useRef(false);
  const [starting, setStarting] = useState(false);
  const startingRef = useRef(false);
  const operationGeneration = useRef(0);
  const [operationID, setOperationID] = useState<string | null>(null);
  const [operation, setOperation] = useState<EngineOperation | null>(null);
  const [operationError, setOperationError] = useState<string | null>(null);
  const [ticket, setTicket] = useState<SensitiveTicket | null>(null);
  const ticketGeneration = useRef(0);
  const exporting = useRef(false);
  const [pendingApproveDevice, setPendingApproveDevice] = useState<EngineDevice | null>(null);
  const [selectedDeviceID, setSelectedDeviceID] = useState<string | null>(null);
  const [selectedCandidate, setSelectedCandidate] = useState<EngineCandidate | null>(null);
  const [foreground, setForeground] = useState(AppState.currentState === 'active');

  const closeTicket = useCallback(() => {
    ticketGeneration.current++;
    setTicket(null);
  }, []);

  useEffect(() => {
    const subscription = AppState.addEventListener('change', (state) => {
      const active = state === 'active';
      setForeground(active);
      if (!active) closeTicket();
    });
    return () => subscription.remove();
  }, [closeTicket]);

  useEffect(() => {
    if (focused && mode === 'live') return;
    setManualJoinOpen(false);
    closeTicket();
  }, [focused, mode, closeTicket]);

  useEffect(() => {
    if (!ticket) return;
    const timer = setTimeout(closeTicket, Math.max(0, ticket.expires_at_ms - Date.now()));
    return () => clearTimeout(timer);
  }, [ticket, closeTicket]);

  useEffect(() => {
    if (!operationID || mode !== 'live') return;
    let request = 0;
    let disposed = false;
    const refresh = async () => {
      const current = ++request;
      try {
        const result = await unicEngineNative.getOperation(operationID);
        if (!disposed && current === request) {
          setOperation(result);
          setOperationError(null);
        }
      } catch (error) {
        if (!disposed && current === request) setOperationError(parseEngineError(error).code);
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
  }, [operationID, mode]);

  const errorText = (error: unknown) => {
    const code = parseEngineError(error).code;
    return t(`errors:engine.${code}`, { defaultValue: t('errors:engine.internal') });
  };

  const run = async (action: () => Promise<unknown>) => {
    try {
      await action();
    } catch (error) {
      Alert.alert(pair('actionFailed'), errorText(error));
    }
  };

  const start = async (action: () => Promise<string>) => {
    if (startingRef.current) return;
    startingRef.current = true;
    const generation = ++operationGeneration.current;
    setStarting(true);
    setOperationID(null);
    setOperation(null);
    setOperationError(null);
    try {
      const id = await action();
      if (generation === operationGeneration.current) setOperationID(id);
    } catch (error) {
      if (generation === operationGeneration.current) {
        Alert.alert(pair('actionFailed'), errorText(error));
      }
    } finally {
      startingRef.current = false;
      setStarting(false);
    }
  };

  const submit = (input: string) => {
    const raw = input.trim();
    const digits = raw.replace(/\s+/g, '');
    const store = useEngineStore.getState();
    setManualJoinOpen(false);
    if (/^\d{3}-?\d{3}$/.test(digits)) {
      void start(() => store.joinByCode(digits));
    } else if (raw.startsWith('unic-pair:')) {
      void start(() => store.joinByTicket(raw));
    } else {
      Alert.alert(pair('actionFailed'), pair('invalidInput'));
    }
  };

  const handleScanJoin = async () => {
    if (scanningRef.current) return;
    scanningRef.current = true;
    try {
      const scanned = await unicEngineNative.scanQRCode(
        t('common:action.cancel'),
        t('settingsSync:lan.qr.hint')
      );
      if (scanned) submit(scanned);
    } catch (error) {
      const parsed = parseEngineError(error);
      if (parsed.code === 'permission_denied') {
        Alert.alert(t('settingsSync:lan.qr.permissionTitle'), pair('cameraPermission'), [
          { text: t('common:action.cancel'), style: 'cancel' },
          { text: pair('openSettings'), onPress: () => void Linking.openSettings() },
        ]);
      } else if (parsed.code === 'unsupported') {
        Alert.alert(pair('actionFailed'), t('home:toast.cameraUnavailable'));
      } else {
        Alert.alert(pair('actionFailed'), errorText(error));
      }
    } finally {
      scanningRef.current = false;
    }
  };

  const exportTicket = async () => {
    if (exporting.current) return;
    exporting.current = true;
    const generation = ++ticketGeneration.current;
    try {
      const result = await useEngineStore.getState().exportPairingTicket();
      if (generation === ticketGeneration.current && result.expires_at_ms > Date.now()) {
        setTicket(result);
      }
    } catch (error) {
      if (generation === ticketGeneration.current) {
        Alert.alert(pair('actionFailed'), errorText(error));
      }
    } finally {
      exporting.current = false;
    }
  };

  const store = useEngineStore.getState();
  const trusted = devices.find((d) => d.trust === 'trusted' && d.id === selectedDeviceID) ?? null;
  const peer = devices.find((d) => d.id === operation?.pairing?.peer_id);
  const waitingApproval = Boolean(
    operation?.pairing &&
      ((operation.pairing.awaiting_local_approval && peer?.trust !== 'trusted') ||
        (operation.pairing.awaiting_remote_approval &&
          !peer?.bindings?.some((binding) => binding.connection === 'connected')))
  );
  const phase = waitingApproval ? 'waitingApproval' : (operation?.phase ?? 'resolving');
  const operationMessage = operationError || operation?.error?.code;
  const operationStatus = operationMessage
    ? t(`errors:engine.${operationMessage}`, { defaultValue: t('errors:engine.internal') })
    : t(`settingsSync:pairing.phase.${phase}`, { defaultValue: pair('phase.resolving') });

  return (
    <>
      <Host style={styles.host}>
        <ZStack modifiers={[fillModifier, ...(iosAccentColor ? [tint(iosAccentColor)] : [])]}>
          <IosPageChromeProvider value={NAVIGATION_CHROME}>
            <NavigationStack path={path} onPathChange={setPath}>
              <DevicesRootPage
                mode={mode === 'live' ? 'live' : 'unavailable'}
                self={self}
                devices={devices}
                candidates={candidates}
                pairingCode={pairingCode}
                starting={starting}
                operationID={operationID}
                operationStatus={operationStatus}
                onRefresh={() => void run(store.refreshDevicesAndCandidates)}
                onCopyLocalID={() => {
                  if (!self) return;
                  void run(async () => {
                    await unicEngineNative.copyPlainText(self.id);
                    useMessageStore.getState().showMessage(t('errors:copy.copied'), 'success');
                  });
                }}
                onCreatePairingCode={() => void run(store.createPairingCode)}
                onCancelPairingCode={() => void run(store.cancelPairingCode)}
                onOpenManualJoin={() => setManualJoinOpen(true)}
                onOpenScanJoin={() => void handleScanJoin()}
                onExportTicket={() => void exportTicket()}
                onStopWaiting={() => {
                  operationGeneration.current++;
                  setStarting(false);
                  setOperationID(null);
                  setOperation(null);
                  setOperationError(null);
                }}
                onApprovePending={setPendingApproveDevice}
                onSelectDevice={(device) => setSelectedDeviceID(device.id)}
                onSelectCandidate={setSelectedCandidate}
                onOpenConnectionPreview={() => navigation.navigate('ConnectionPreview')}
                onShowUnavailable={showUnavailable}
              />
            </NavigationStack>
          </IosPageChromeProvider>

          <DeviceDetailSheet
            device={trusted}
            onClose={() => setSelectedDeviceID(null)}
            onRename={async (device, nextName) => {
              await run(async () => {
                await store.renameDevice(device.id, device.revision, nextName);
                setSelectedDeviceID(null);
              });
            }}
            onToggleSync={async (binding, enabled) => {
              await run(() => store.setBindingSync(binding.id, binding.revision, enabled));
            }}
            onUnpair={async (device) => {
              setSelectedDeviceID(null);
              await run(() => store.unpairDevice(device.id, device.revision));
            }}
          />

          <PairingJoinSheet
            visible={manualJoinOpen}
            starting={starting}
            onSubmitInput={submit}
            onClose={() => setManualJoinOpen(false)}
          />

          <PairingTicketSheet
            ticket={ticket}
            focused={focused}
            foreground={foreground}
            onCopy={(activeTicket) => {
              void run(async () => {
                if (activeTicket.expires_at_ms <= Date.now()) {
                  closeTicket();
                  return;
                }
                await unicEngineNative.copyPairingTicket(
                  activeTicket.ticket,
                  activeTicket.expires_at_ms
                );
                Alert.alert(t('errors:copy.copied'));
              });
            }}
            onClose={closeTicket}
          />

          <CandidateDetailSheet
            candidate={selectedCandidate}
            starting={starting}
            onConnect={(candidate) => {
              setSelectedCandidate(null);
              void start(() => store.pairCandidate(candidate));
            }}
            onIgnore={(candidate) => {
              setSelectedCandidate(null);
              void run(() => store.ignoreCandidate(candidate.device_id, true));
            }}
            onClose={() => setSelectedCandidate(null)}
          />
        </ZStack>
      </Host>

      <AppAlertDialog
        visible={pendingApproveDevice !== null}
        standalone
        title={pair('approveTitle')}
        message={
          pendingApproveDevice
            ? pair('approveBody', {
                name: deviceDisplayName(pendingApproveDevice),
                value: pendingApproveDevice.fingerprint,
              })
            : ''
        }
        confirmLabel={pair('approve')}
        onConfirm={() => {
          const target = pendingApproveDevice;
          setPendingApproveDevice(null);
          if (target) void run(() => store.approveDevice(target, target.fingerprint));
        }}
        dismissLabel={t('common:action.cancel')}
        onDismiss={() => setPendingApproveDevice(null)}
        onDismissAction={() => setPendingApproveDevice(null)}
      />
    </>
  );
}

const styles = StyleSheet.create({
  host: { flex: 1 },
});
