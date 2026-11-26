import { useEffect, useState } from 'react';
import { Alert, AppState, Text, TextInput, View } from 'react-native';
import { useIsFocused, useNavigation } from '@react-navigation/native';
import type { NativeStackNavigationProp } from '@react-navigation/native-stack';
import { useTranslation } from 'react-i18next';

import { AppBottomSheet, AppButton, AppHost } from '@/components/ui';
import { parseEngineError } from '@/engine/nativeModule';
import { useEngineStore } from '@/engine/store';
import type { EngineCandidate } from '@/engine/types';
import { useTheme } from '@/hooks/useTheme';
import type { RootStackParamList, SpaceDeviceTarget } from '@/navigation/AppNavigator.types';
import { PairingFlowModal } from '@/screens/pairing/PairingFlowModal';
import { usePairingUIStore } from '@/screens/pairing/pairingStore';
import {
  deviceDisplayName,
  deviceStatusKind,
  formatFingerprintLines,
  formatFingerprintShort,
  needsDeviceApproval,
  sortTrustedDevices,
} from '@/screens/pairing/pairingUtils';
import { usePairingFlow } from '@/screens/pairing/usePairingFlow';
import { SettingsPage } from '@/screens/settings/SettingsPage';
import { showUnavailable } from '@/shell/notice';
import { PairingScanner } from './PairingScanner';

export function DevicesScreen(_props: SpaceDeviceTarget) {
  const { t } = useTranslation();
  const pair = (key: string, values?: Record<string, string | number>) =>
    t(`settingsSync:pairing.${key}`, values);
  const { theme } = useTheme();
  const navigation = useNavigation<NativeStackNavigationProp<RootStackParamList>>();
  const focused = useIsFocused();

  const mode = useEngineStore((s) => s.mode);
  const self = useEngineStore((s) => s.snapshot?.self);
  const devices = useEngineStore((s) => s.devices);
  const candidates = useEngineStore((s) => s.candidates);

  const flow = usePairingFlow(focused);
  const [scanOpen, setScanOpen] = useState(false);
  const [foreground, setForeground] = useState(AppState.currentState === 'active');
  const [selectedDeviceID, setSelectedDeviceID] = useState<string | null>(null);
  const [selectedCandidate, setSelectedCandidate] = useState<EngineCandidate | null>(null);
  const [name, setName] = useState('');

  useEffect(() => {
    const sub = AppState.addEventListener('change', (state) => {
      const active = state === 'active';
      setForeground(active);
      if (!active) setScanOpen(false);
    });
    return () => sub.remove();
  }, []);

  useEffect(() => {
    if (!focused || mode !== 'live') setScanOpen(false);
  }, [focused, mode]);

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

  if (mode !== 'live') {
    return (
      <SettingsPage
        title={t('home:nav.devices')}
        groups={[
          {
            title: t('settings:syncChannel.title'),
            footer: t('shell:unavailable'),
            rows: [
              {
                id: 'devices-add',
                title: t('shell:addConnection'),
                onPress: () => navigation.navigate('ConnectionPreview'),
              },
            ],
          },
          {
            title: t('shell:demoDevices'),
            rows: ['demoMac', 'demoPhone'].map((device, index) => ({
              id: `device-${index}`,
              title: pair(device),
              value: t('shell:disconnected'),
              onPress: showUnavailable,
            })),
          },
        ]}
      />
    );
  }

  const store = useEngineStore.getState();
  const pendingDevices = devices.filter(needsDeviceApproval);
  const trustedDevices = sortTrustedDevices(devices.filter((d) => d.trust === 'trusted'));
  const trusted = trustedDevices.find((d) => d.id === selectedDeviceID);

  const titleStyle = { color: theme.colors.textPrimary, fontSize: 18, fontWeight: '700' as const };
  const inputStyle = {
    borderRadius: 12,
    padding: 12,
    backgroundColor: theme.colors.surface,
    color: theme.colors.textPrimary,
  };

  return (
    <>
      <SettingsPage
        title={t('home:nav.devices')}
        groups={[
          {
            title: pair('newDevice'),
            footer: pair('overviewEmptyBody'),
            rows: [
              { id: 'devices-scan', title: pair('scanShort'), onPress: () => setScanOpen(true) },
              {
                id: 'devices-join',
                title: pair('codeShort'),
                onPress: () => flow.openCodeSheet('enter'),
              },
            ],
          },
          ...(pendingDevices.length
            ? [
                {
                  title: `${pair('pending')} · ${pendingDevices.length}`,
                  rows: pendingDevices.map((device) => ({
                    id: `pending-${device.id}`,
                    title: deviceDisplayName(device),
                    description: formatFingerprintShort(device.fingerprint),
                    value: pair('reviewNow'),
                    onPress: () => usePairingUIStore.getState().openManualApproval(device.id),
                  })),
                },
              ]
            : []),
          ...(trustedDevices.length
            ? [
                {
                  title: `${pair('trusted')} · ${trustedDevices.length}`,
                  rows: trustedDevices.map((device) => ({
                    id: `device-${device.id}`,
                    title: deviceDisplayName(device),
                    value: pair(deviceStatusKind(device)),
                    onPress: () => {
                      setName(device.name || device.reported_name || '');
                      setSelectedDeviceID(device.id);
                    },
                  })),
                },
              ]
            : []),
          ...(candidates.length
            ? [
                {
                  title: `${pair('recommended')} · ${candidates.length}`,
                  rows: candidates.map((candidate) => ({
                    id: `candidate-${candidate.device_id}`,
                    title: candidate.name || candidate.device_id.slice(0, 8),
                    value: pair(candidate.requested ? 'phase.connecting' : 'reviewNow'),
                    onPress: () => setSelectedCandidate(candidate),
                  })),
                },
              ]
            : []),
          ...(self
            ? [
                {
                  title: pair('thisDevice'),
                  rows: [
                    {
                      id: 'devices-local-identity',
                      title: self.name,
                      description: pair('deviceID', { id: self.id }),
                    },
                  ],
                },
              ]
            : []),
        ]}
      />

      <PairingFlowModal
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
          setScanOpen(true);
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

      <AppBottomSheet visible={scanOpen} onDismiss={() => setScanOpen(false)}>
        <View style={{ padding: 16, gap: 12 }}>
          <Text style={titleStyle}>{pair('scanShort')}</Text>
          {scanOpen && focused && foreground ? (
            <PairingScanner
              onRead={(value) => {
                setScanOpen(false);
                flow.submitInput(value);
              }}
            />
          ) : null}
          <AppHost matchContents={{ vertical: true }}>
            <AppButton
              title={t('common:action.cancel')}
              variant="text"
              fullWidth
              onPress={() => setScanOpen(false)}
            />
          </AppHost>
        </View>
      </AppBottomSheet>

      <AppBottomSheet
        visible={selectedCandidate !== null}
        onDismiss={() => setSelectedCandidate(null)}
      >
        {selectedCandidate ? (
          <View style={{ padding: 16, gap: 12 }}>
            <Text style={titleStyle}>
              {selectedCandidate.name || selectedCandidate.device_id.slice(0, 8)}
            </Text>
            <AppHost matchContents={{ vertical: true }}>
              <AppButton
                title={pair('connect')}
                fullWidth
                disabled={flow.stage === 'connecting' || selectedCandidate.requested}
                onPress={() => {
                  const target = selectedCandidate;
                  setSelectedCandidate(null);
                  flow.pairCandidate(target);
                }}
              />
            </AppHost>
            <AppHost matchContents={{ vertical: true }}>
              <AppButton
                title={pair('ignore')}
                variant="outlined"
                fullWidth
                onPress={() => {
                  const target = selectedCandidate;
                  setSelectedCandidate(null);
                  void run(() => store.ignoreCandidate(target.device_id, true));
                }}
              />
            </AppHost>
          </View>
        ) : null}
      </AppBottomSheet>

      <AppBottomSheet visible={trusted !== undefined} onDismiss={() => setSelectedDeviceID(null)}>
        {trusted ? (
          <View style={{ padding: 16, gap: 12 }}>
            <Text style={titleStyle}>{deviceDisplayName(trusted)}</Text>
            <Text style={{ color: theme.colors.textSecondary }}>
              {formatFingerprintLines(trusted.fingerprint)}
            </Text>
            <TextInput
              testID="devices-rename-input"
              accessibilityLabel={pair('rename')}
              value={name}
              onChangeText={setName}
              style={inputStyle}
            />
            <AppHost matchContents={{ vertical: true }}>
              <AppButton
                title={t('common:action.save')}
                variant="filled"
                fullWidth
                disabled={!name.trim() || name.trim() === trusted.name}
                onPress={() => {
                  void run(async () => {
                    await store.renameDevice(trusted.id, trusted.revision, name.trim());
                    setSelectedDeviceID(null);
                  });
                }}
              />
            </AppHost>
            {trusted.bindings?.map((binding) => (
              <AppHost key={binding.id} matchContents={{ vertical: true }}>
                <AppButton
                  title={pair(binding.sync_enabled ? 'pauseSync' : 'resumeSync')}
                  variant="tonal"
                  fullWidth
                  onPress={() =>
                    void run(() =>
                      store.setBindingSync(binding.id, binding.revision, !binding.sync_enabled),
                    )
                  }
                />
              </AppHost>
            ))}
            <AppHost matchContents={{ vertical: true }}>
              <AppButton
                title={pair('unpair')}
                variant="outlined"
                fullWidth
                onPress={() => {
                  setSelectedDeviceID(null);
                  void run(() => store.unpairDevice(trusted.id, trusted.revision));
                }}
              />
            </AppHost>
            <AppHost matchContents={{ vertical: true }}>
              <AppButton
                title={t('common:action.close')}
                variant="text"
                fullWidth
                onPress={() => setSelectedDeviceID(null)}
              />
            </AppHost>
          </View>
        ) : null}
      </AppBottomSheet>
    </>
  );
}
