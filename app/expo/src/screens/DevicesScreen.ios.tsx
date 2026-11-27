import { useRef, useState } from 'react';
import { Alert, Linking, StyleSheet } from 'react-native';
import { useIsFocused, useNavigation } from '@react-navigation/native';
import type { NativeStackNavigationProp } from '@react-navigation/native-stack';
import { Host, NavigationStack, ZStack } from '@expo/ui/swift-ui';
import { frame, tint } from '@expo/ui/swift-ui/modifiers';
import { useTranslation } from 'react-i18next';

import {
  IosPageChromeProvider,
  type IosPageChrome,
} from '@/components/ui';
import { parseEngineError, unicEngineNative } from '@/engine/nativeModule';
import { useEngineStore } from '@/engine/store';
import type { EngineCandidate } from '@/engine/types';
import type { RootStackParamList, SpaceDeviceTarget } from '@/navigation/AppNavigator.types';
import { useHideTabBarOnSubPage } from '@/navigation/ios/useHideTabBarOnSubPage';
import { usePairingUIStore } from '@/screens/pairing/pairingStore';
import { usePairingFlow } from '@/screens/pairing/usePairingFlow';
import { showUnavailable } from '@/shell/notice';
import { useMessageStore } from '@/stores/messageStore';
import { iosAccentColor } from '@/theme/iosDesignTokens';
import { DeviceDetailSheet } from './ios/devices/DeviceDetailSheet';
import { DevicesRootPage } from './ios/devices/DevicesRootPage';
import { CandidateDetailSheet, PairingFlowSheet } from './ios/devices/PairingSheets';

const fillModifier = frame({ maxWidth: Infinity, maxHeight: Infinity });
const NAVIGATION_CHROME: IosPageChrome = { kind: 'navigation' };

/**
 * iOS "Devices" tab.
 * Full-screen SwiftUI Host owning the `NavigationStack` and sibling native sheets.
 * Incoming pairing approvals are auto-surfaced either inline inside `PairingFlowSheet`
 * (when actively pairing) or globally via `PairingApprovalHost`.
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

  const flow = usePairingFlow(focused);
  const scanningRef = useRef(false);
  const [selectedDeviceID, setSelectedDeviceID] = useState<string | null>(null);
  const [selectedCandidate, setSelectedCandidate] = useState<EngineCandidate | null>(null);

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

  const handleScanJoin = async () => {
    if (scanningRef.current) return;
    scanningRef.current = true;
    try {
      const scanned = await unicEngineNative.scanQRCode(
        t('common:action.cancel'),
        t('settingsSync:lan.qr.hint'),
      );
      if (scanned) flow.submitInput(scanned);
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

  const store = useEngineStore.getState();
  const trusted = devices.find((d) => d.trust === 'trusted' && d.id === selectedDeviceID) ?? null;

  return (
    <Host style={styles.host}>
      <ZStack modifiers={[fillModifier, ...(iosAccentColor ? [tint(iosAccentColor)] : [])]}>
        <IosPageChromeProvider value={NAVIGATION_CHROME}>
          <NavigationStack path={path} onPathChange={setPath}>
            <DevicesRootPage
              mode={mode === 'live' ? 'live' : 'unavailable'}
              self={self}
              devices={devices}
              candidates={candidates}
              pairingCode={flow.pairingCode}
              pairingCodeCountdown={flow.countdown}
              onRefresh={() => void run(store.refreshDevicesAndCandidates)}
              onCopyLocalID={() => {
                if (!self) return;
                void run(async () => {
                  await unicEngineNative.copyPlainText(self.id);
                  useMessageStore.getState().showMessage(t('errors:copy.copied'), 'success');
                });
              }}
              onOpenScanJoin={() => void handleScanJoin()}
              onOpenCodeSheet={(initialTab) => flow.openCodeSheet(initialTab)}
              onReviewPending={(device) =>
                usePairingUIStore.getState().openManualApproval(device.id)
              }
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

        <PairingFlowSheet
          visible={flow.open}
          tab={flow.tab}
          stage={flow.stage}
          busy={flow.busy}
          errorMessage={flow.errorMessage}
          creatingCode={flow.creatingCode}
          codeError={flow.codeError}
          pairingCode={flow.pairingCode}
          countdown={flow.countdown}
          ticket={flow.ticket}
          exportingTicket={flow.exportingTicket}
          verifyDevice={flow.verifyDevice}
          approvedPeerName={flow.approvedPeer?.name ?? ''}
          handshakeStageIndex={flow.handshakeStageIndex}
          onSelectTab={flow.selectTab}
          onSubmitInput={flow.submitInput}
          onSwitchToScan={() => {
            flow.close();
            void handleScanJoin();
          }}
          onRegenerateCode={() => void flow.generateCode()}
          onCancelCode={() => void flow.cancelCode()}
          onCopyCode={() => void flow.copyPairingCode()}
          onExportTicket={() => void flow.exportOfflineTicket()}
          onCopyTicket={() => void flow.copyOfflineTicket()}
          onApprove={() => void flow.approveVerifyDevice()}
          onReject={() => void flow.rejectVerifyDevice()}
          onRetryEntry={flow.retryEntry}
          onClose={flow.close}
        />

        <CandidateDetailSheet
          candidate={selectedCandidate}
          starting={flow.stage === 'connecting'}
          onConnect={(candidate) => {
            setSelectedCandidate(null);
            flow.pairCandidate(candidate);
          }}
          onIgnore={(candidate) => {
            setSelectedCandidate(null);
            void run(() => store.ignoreCandidate(candidate.device_id, true));
          }}
          onClose={() => setSelectedCandidate(null)}
        />
      </ZStack>
    </Host>
  );
}

const styles = StyleSheet.create({
  host: { flex: 1 },
});
