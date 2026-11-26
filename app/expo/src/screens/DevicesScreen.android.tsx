/**
 * Devices management screen (Android — Material 3 Expressive).
 *
 * Simplified mental model:
 * 1. Top 28dp Status Overview Hero card with two clear actions ("Scan QR" & "Pairing code")
 *    plus an active pairing-code strip when a code is live.
 * 2. Pending approval devices (if any remain after auto-popup dismissal).
 * 3. Paired devices (sorted online first; clean detail sheet with sync switch & confirmed unpair).
 * 4. Suggested devices introduced by trusted peers (when present).
 * 5. Compact "This device" row at the bottom (tap to copy ID).
 */
import { useCallback, useEffect, useLayoutEffect, useState } from 'react';
import { Alert, AppState, StyleSheet, Text, TextInput, View } from 'react-native';
import { useIsFocused, useNavigation } from '@react-navigation/native';
import type { NativeStackNavigationProp } from '@react-navigation/native-stack';
import {
  Box,
  Button,
  Column,
  FilledTonalButton,
  Host,
  Icon,
  LazyColumn,
  ListItem,
  Row,
  Shape,
  Spacer,
  Surface,
  Text as ComposeText,
  useMaterialColors,
} from '@expo/ui/jetpack-compose';
import {
  clickable,
  fillMaxSize,
  fillMaxWidth,
  height as heightModifier,
  padding,
  size,
  testID,
  weight,
  width as widthModifier,
} from '@expo/ui/jetpack-compose/modifiers';
import { Laptop, ShieldCheck } from 'lucide-react-native';
import { SafeAreaView, useSafeAreaInsets } from 'react-native-safe-area-context';
import { useTranslation } from 'react-i18next';

import { useFloatingNavigationClearance } from '@/components/android/floatingNavigationClearance';
import { M3IconButton } from '@/components/android/M3IconButton';
import { AppAlertDialog, AppBottomSheet, AppButton, AppHost, AppSwitch } from '@/components/ui';
import { parseEngineError, unicEngineNative } from '@/engine/nativeModule';
import { useEngineStore } from '@/engine/store';
import type { EngineCandidate, EngineDevice } from '@/engine/types';
import { useTheme } from '@/hooks/useTheme';
import type { RootStackParamList, SpaceDeviceTarget } from '@/navigation/AppNavigator.types';
import { PairingFlowModal } from '@/screens/pairing/PairingFlowModal';
import { usePairingUIStore } from '@/screens/pairing/pairingStore';
import {
  deviceDisplayName,
  deviceStatusKind,
  formatFingerprintLines,
  formatFingerprintShort,
  formatPairingCodeDisplay,
  needsDeviceApproval,
  sortTrustedDevices,
} from '@/screens/pairing/pairingUtils';
import { usePairingFlow } from '@/screens/pairing/usePairingFlow';
import { SettingsSectionItem, useSettingsSectionRowColors } from '@/screens/settings/SettingsSectionItem';
import {
  SettingsLeadingIcon,
  type SettingsLeadingIconTone,
} from '@/screens/settings/android/SettingsLeadingIcon';
import { SettingsListRow } from '@/screens/settings/android/SettingsListRow';
import { showUnavailable } from '@/shell/notice';
import { useMessageStore } from '@/stores/messageStore';
import { MATERIAL_SEED_COLOR } from '@/theme/colors';
import { PairingScanner } from './PairingScanner';

const HERO_TITLE_STYLE = { fontSize: 20, fontWeight: '500', letterSpacing: 0 } as const;
const CODE_STYLE = { fontSize: 22, fontWeight: '700', letterSpacing: 2 } as const;
const HERO_SHAPE = Shape.RoundedCorner({
  cornerRadii: { topStart: 28, topEnd: 28, bottomStart: 28, bottomEnd: 28 },
});
const HERO_BADGE_SHAPE = Shape.RoundedCorner({
  cornerRadii: { topStart: 22, topEnd: 22, bottomStart: 22, bottomEnd: 22 },
});

const ICONS = {
  add: require('../assets/icons/add.xml'),
  alert: require('../assets/icons/info.xml'),
  check: require('../assets/icons/check.xml'),
  chevron: require('../assets/icons/chevron_right.xml'),
  copy: require('../assets/icons/content_copy.xml'),
  device: require('../assets/icons/devices.xml'),
  join: require('../assets/icons/account_circle.xml'),
  phone: require('../assets/icons/smartphone.xml'),
  qr: require('../assets/icons/qr_code_scanner.xml'),
  status: require('../assets/icons/circle.xml'),
};

function LocalDeviceRow({
  name,
  idLabel,
  onCopy,
}: {
  name: string;
  idLabel: string;
  onCopy: () => void;
}) {
  const { t } = useTranslation();
  const colors = useMaterialColors();
  const rowColors = useSettingsSectionRowColors();
  const { theme } = useTheme();
  return (
    <ListItem
      colors={rowColors}
      modifiers={[testID('devices-local-identity'), clickable(onCopy)]}
    >
      <ListItem.LeadingContent>
        <SettingsLeadingIcon source={ICONS.phone} tone="primary" />
      </ListItem.LeadingContent>
      <ListItem.HeadlineContent>
        <ComposeText>{name}</ComposeText>
      </ListItem.HeadlineContent>
      <ListItem.SupportingContent>
        <Row verticalAlignment="center">
          <Icon source={ICONS.status} size={8} tint={theme.colors.success as string} />
          <Spacer modifiers={[widthModifier(6)]} />
          <ComposeText color={colors.onSurfaceVariant}>{idLabel}</ComposeText>
        </Row>
      </ListItem.SupportingContent>
      <ListItem.TrailingContent>
        <Icon
          source={ICONS.copy}
          size={20}
          tint={colors.onSurfaceVariant}
          contentDescription={t('common:action.copy')}
        />
      </ListItem.TrailingContent>
    </ListItem>
  );
}

function TrustedDeviceRow({
  device,
  statusText,
  onSelect,
}: {
  device: EngineDevice;
  statusText: string;
  onSelect: () => void;
}) {
  const colors = useMaterialColors();
  const rowColors = useSettingsSectionRowColors();
  const { theme } = useTheme();
  const kind = deviceStatusKind(device);
  const connected = kind === 'connected';
  const statusColor = connected
    ? (theme.colors.success as string)
    : kind === 'paused' || kind === 'connecting'
      ? (theme.colors.warning as string)
      : colors.outline;
  const iconTone: SettingsLeadingIconTone = connected ? 'primary' : 'muted';

  return (
    <ListItem
      colors={rowColors}
      modifiers={[testID(`device-${device.id}`), clickable(onSelect)]}
    >
      <ListItem.LeadingContent>
        <SettingsLeadingIcon source={ICONS.device} tone={iconTone} />
      </ListItem.LeadingContent>
      <ListItem.HeadlineContent>
        <ComposeText>{deviceDisplayName(device)}</ComposeText>
      </ListItem.HeadlineContent>
      <ListItem.SupportingContent>
        <Row verticalAlignment="center">
          <Icon source={ICONS.status} size={8} tint={statusColor} />
          <Spacer modifiers={[widthModifier(6)]} />
          <ComposeText color={statusColor}>{statusText}</ComposeText>
        </Row>
      </ListItem.SupportingContent>
      <ListItem.TrailingContent>
        <Icon source={ICONS.chevron} size={20} tint={colors.onSurfaceVariant} />
      </ListItem.TrailingContent>
    </ListItem>
  );
}

function DevicesOverviewHero({
  pendingCount,
  onlineCount,
  offlineCount,
  pairingCode,
  pairingCodeCountdown,
  onReviewPending,
  onOpenScan,
  onOpenCodeSheet,
}: {
  pendingCount: number;
  onlineCount: number;
  offlineCount: number;
  pairingCode: string | null;
  pairingCodeCountdown: string;
  onReviewPending: () => void;
  onOpenScan: () => void;
  onOpenCodeSheet: (tab: 'enter' | 'show') => void;
}) {
  const { t } = useTranslation();
  const pair = (key: string, values?: Record<string, string | number>) =>
    t(`settingsSync:pairing.${key}`, values);
  const colors = useMaterialColors();
  const hasPending = pendingCount > 0;
  const totalTrusted = onlineCount + offlineCount;
  const healthy = !hasPending && onlineCount > 0;

  const heroContainer = hasPending
    ? colors.errorContainer
    : healthy
      ? colors.primaryContainer
      : colors.surfaceContainerHigh;
  const heroContent = hasPending
    ? colors.onErrorContainer
    : healthy
      ? colors.onPrimaryContainer
      : colors.onSurface;
  const heroBadgeContainer = hasPending
    ? colors.error
    : healthy
      ? colors.onPrimaryContainer
      : colors.secondaryContainer;
  const heroBadgeContent = hasPending
    ? colors.onError
    : healthy
      ? colors.primaryContainer
      : colors.onSecondaryContainer;

  const overviewTitle = hasPending
    ? pair('overviewPendingTitle')
    : healthy
      ? pair('overviewReadyTitle')
      : totalTrusted > 0
        ? pair('overviewOfflineTitle')
        : pair('overviewEmptyTitle');

  const overviewBody = hasPending
    ? pair('overviewPendingBody', { count: pendingCount })
    : onlineCount > 0
      ? offlineCount > 0
        ? t('settingsSync:space.overview.deviceSummary', {
            online: onlineCount,
            offline: offlineCount,
          })
        : pair('overviewOnlineOnly', { count: onlineCount })
      : totalTrusted > 0
        ? pair('overviewAllOffline', { count: totalTrusted })
        : pair('overviewEmptyBody');

  return (
    <Surface
      color={heroContainer}
      contentColor={heroContent}
      shape={HERO_SHAPE}
      modifiers={[fillMaxWidth()]}
    >
      <Column modifiers={[fillMaxWidth(), padding(20, 20, 20, 20)]}>
        <Row verticalAlignment="top" modifiers={[fillMaxWidth()]}>
          <Surface
            color={heroBadgeContainer}
            shape={HERO_BADGE_SHAPE}
            modifiers={[size(44, 44)]}
          >
            <Box contentAlignment="center" modifiers={[size(44, 44)]}>
              <Icon
                source={hasPending ? ICONS.alert : healthy ? ICONS.check : ICONS.device}
                size={24}
                tint={heroBadgeContent}
              />
            </Box>
          </Surface>
          <Spacer modifiers={[widthModifier(14)]} />
          <Column modifiers={[weight(1)]}>
            <ComposeText color={heroContent} style={HERO_TITLE_STYLE}>
              {overviewTitle}
            </ComposeText>
            <Spacer modifiers={[heightModifier(2)]} />
            <ComposeText color={heroContent}>{overviewBody}</ComposeText>
          </Column>
        </Row>

        {hasPending ? (
          <>
            <Spacer modifiers={[heightModifier(14)]} />
            <Button
              modifiers={[fillMaxWidth(), testID('devices-review-now')]}
              onClick={onReviewPending}
            >
              <ComposeText maxLines={1}>{pair('reviewNow')}</ComposeText>
            </Button>
          </>
        ) : null}

        {pairingCode ? (
          <>
            <Spacer modifiers={[heightModifier(14)]} />
            <Surface
              color={colors.surfaceContainerLowest}
              shape={Shape.RoundedCorner({
                cornerRadii: { topStart: 16, topEnd: 16, bottomStart: 16, bottomEnd: 16 },
              })}
              modifiers={[
                fillMaxWidth(),
                testID('devices-active-code-banner'),
                clickable(() => onOpenCodeSheet('show')),
              ]}
            >
              <Row
                verticalAlignment="center"
                modifiers={[fillMaxWidth(), padding(16, 12, 16, 12)]}
              >
                <Column modifiers={[weight(1)]}>
                  <ComposeText color={colors.onSurfaceVariant}>
                    {pairingCodeCountdown
                      ? pair('activeCodeBanner', { time: pairingCodeCountdown })
                      : pair('myCodeTab')}
                  </ComposeText>
                  <ComposeText color={colors.primary} style={CODE_STYLE}>
                    {formatPairingCodeDisplay(pairingCode)}
                  </ComposeText>
                </Column>
                <Icon source={ICONS.chevron} size={20} tint={colors.onSurfaceVariant} />
              </Row>
            </Surface>
          </>
        ) : null}

        <Spacer modifiers={[heightModifier(16)]} />
        <Row verticalAlignment="center" modifiers={[fillMaxWidth()]}>
          <Box modifiers={[weight(1)]}>
            <Button
              modifiers={[fillMaxWidth(), testID('devices-scan')]}
              onClick={onOpenScan}
            >
              <Icon source={ICONS.qr} size={18} tint={colors.onPrimary} />
              <Spacer modifiers={[widthModifier(6)]} />
              <ComposeText maxLines={1}>{pair('scanShort')}</ComposeText>
            </Button>
          </Box>
          <Spacer modifiers={[widthModifier(10)]} />
          <Box modifiers={[weight(1)]}>
            <FilledTonalButton
              modifiers={[fillMaxWidth(), testID('devices-join')]}
              onClick={() => onOpenCodeSheet('enter')}
            >
              <Icon source={ICONS.join} size={18} tint={colors.onSecondaryContainer} />
              <Spacer modifiers={[widthModifier(6)]} />
              <ComposeText maxLines={1}>{pair('codeShort')}</ComposeText>
            </FilledTonalButton>
          </Box>
        </Row>
      </Column>
    </Surface>
  );
}

export function DevicesScreen(_props: SpaceDeviceTarget) {
  const { t } = useTranslation();
  const pair = (key: string, values?: Record<string, string | number>) =>
    t(`settingsSync:pairing.${key}`, values);
  const { theme } = useTheme();
  const c = theme.colors;
  const insets = useSafeAreaInsets();
  const navClearance = useFloatingNavigationClearance();
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
  const [confirmUnpairDevice, setConfirmUnpairDevice] = useState<EngineDevice | null>(null);

  const errorText = useCallback(
    (error: unknown) => {
      const code = parseEngineError(error).code;
      return t(`errors:engine.${code}`, { defaultValue: t('errors:engine.internal') });
    },
    [t],
  );

  const run = useCallback(
    async (action: () => Promise<unknown>) => {
      try {
        await action();
      } catch (error) {
        Alert.alert(pair('actionFailed'), errorText(error));
      }
    },
    [errorText, t],
  );

  useLayoutEffect(() => {
    if (mode !== 'live') {
      navigation.setOptions({ headerRight: undefined });
      return;
    }
    navigation.setOptions({
      headerRight: () => (
        <M3IconButton
          testID="devices-refresh"
          icon="refresh"
          accessibilityLabel={t('common:action.refresh')}
          onPress={() => void run(useEngineStore.getState().refreshDevicesAndCandidates)}
          colors={theme.colors}
          iconColor={theme.colors.textPrimary}
        />
      ),
    });
    return () => navigation.setOptions({ headerRight: undefined });
  }, [mode, navigation, run, t, theme.colors]);

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

  const contentPadding = {
    start: 16,
    end: 16,
    top: 8,
    bottom: insets.bottom + navClearance + 40,
  };

  if (mode !== 'live') {
    return (
      <SafeAreaView
        style={[styles.container, { backgroundColor: c.background }]}
        edges={[]}
      >
        <Host
          style={styles.container}
          colorScheme={theme.isDark ? 'dark' : 'light'}
          seedColor={MATERIAL_SEED_COLOR}
        >
          <LazyColumn
            modifiers={[fillMaxSize()]}
            contentPadding={contentPadding}
            verticalArrangement={{ spacedBy: 16 }}
          >
            <SettingsSectionItem
              variant="grouped"
              title={t('settings:syncChannel.title')}
              footer={t('shell:unavailable')}
            >
              <SettingsListRow
                key="devices-add"
                testID="devices-add"
                icon={ICONS.add}
                title={t('shell:addConnection')}
                trailing="chevron"
                onPress={() => navigation.navigate('ConnectionPreview')}
              />
            </SettingsSectionItem>
            <SettingsSectionItem variant="grouped" title={t('shell:demoDevices')}>
              {(['demoMac', 'demoPhone'] as const).map((device, index) => (
                <SettingsListRow
                  key={device}
                  testID={`device-${index}`}
                  icon={device === 'demoMac' ? ICONS.device : ICONS.phone}
                  iconTone="muted"
                  title={pair(device)}
                  description={t('shell:disconnected')}
                  trailing="chevron"
                  onPress={showUnavailable}
                />
              ))}
            </SettingsSectionItem>
          </LazyColumn>
        </Host>
      </SafeAreaView>
    );
  }

  const store = useEngineStore.getState();
  const pendingDevices = devices.filter(needsDeviceApproval);
  const trustedDevices = sortTrustedDevices(devices.filter((d) => d.trust === 'trusted'));
  const onlineCount = trustedDevices.filter((d) => deviceStatusKind(d) === 'connected').length;
  const offlineCount = trustedDevices.length - onlineCount;
  const trusted = trustedDevices.find((d) => d.id === selectedDeviceID);

  return (
    <SafeAreaView
      style={[styles.container, { backgroundColor: c.background }]}
      edges={[]}
    >
      <Host
        style={styles.container}
        colorScheme={theme.isDark ? 'dark' : 'light'}
        seedColor={MATERIAL_SEED_COLOR}
      >
        <LazyColumn
          modifiers={[fillMaxSize()]}
          contentPadding={contentPadding}
          verticalArrangement={{ spacedBy: 16 }}
        >
          <DevicesOverviewHero
            pendingCount={pendingDevices.length}
            onlineCount={onlineCount}
            offlineCount={offlineCount}
            pairingCode={flow.pairingCode?.code ?? null}
            pairingCodeCountdown={flow.countdown}
            onReviewPending={() => {
              if (pendingDevices[0]) {
                usePairingUIStore.getState().openManualApproval(pendingDevices[0].id);
              }
            }}
            onOpenScan={() => setScanOpen(true)}
            onOpenCodeSheet={(tab) => flow.openCodeSheet(tab)}
          />

          {pendingDevices.length > 0 ? (
            <SettingsSectionItem variant="grouped" title={`${pair('pending')} · ${pendingDevices.length}`}>
              {pendingDevices.map((device) => (
                <SettingsListRow
                  key={device.id}
                  testID={`pending-${device.id}`}
                  icon={ICONS.alert}
                  tone="warning"
                  title={deviceDisplayName(device)}
                  description={formatFingerprintShort(device.fingerprint)}
                  trailing={{ pill: pair('reviewNow') }}
                  onPress={() => usePairingUIStore.getState().openManualApproval(device.id)}
                />
              ))}
            </SettingsSectionItem>
          ) : null}

          {trustedDevices.length > 0 ? (
            <SettingsSectionItem variant="grouped" title={`${pair('trusted')} · ${trustedDevices.length}`}>
              {trustedDevices.map((device) => (
                <TrustedDeviceRow
                  key={device.id}
                  device={device}
                  statusText={pair(deviceStatusKind(device))}
                  onSelect={() => {
                    setName(device.name || device.reported_name || '');
                    setSelectedDeviceID(device.id);
                  }}
                />
              ))}
            </SettingsSectionItem>
          ) : null}

          {candidates.length > 0 ? (
            <SettingsSectionItem variant="grouped" title={`${pair('recommended')} · ${candidates.length}`}>
              {candidates.map((candidate) => {
                const introducer = candidate.introducers?.[0]?.name;
                return (
                  <SettingsListRow
                    key={candidate.device_id}
                    testID={`candidate-${candidate.device_id}`}
                    icon={ICONS.device}
                    iconTone="muted"
                    title={candidate.name || candidate.device_id.slice(0, 8)}
                    description={
                      candidate.requested
                        ? pair('phase.connecting')
                        : introducer
                          ? pair('recommendedBy', { name: introducer })
                          : pair('recommendedHint')
                    }
                    trailing="chevron"
                    onPress={() => setSelectedCandidate(candidate)}
                  />
                );
              })}
            </SettingsSectionItem>
          ) : null}

          {self ? (
            <SettingsSectionItem variant="grouped" title={pair('thisDevice')}>
              <LocalDeviceRow
                name={self.name}
                idLabel={pair('deviceID', { id: self.id })}
                onCopy={() => {
                  void run(async () => {
                    await unicEngineNative.copyPlainText(self.id);
                    useMessageStore.getState().showMessage(t('errors:copy.copied'), 'success');
                  });
                }}
              />
            </SettingsSectionItem>
          ) : null}
        </LazyColumn>
      </Host>

      {/* Unified Pairing Flow Sheet (Enter Code / This Device's Code / Connecting / Verify / Done) */}
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

      {/* QR Scanner Sheet */}
      <AppBottomSheet visible={scanOpen} onDismiss={() => setScanOpen(false)}>
        <View style={styles.sheetBody}>
          <Text style={[styles.sheetTitle, { color: c.textPrimary }]}>{pair('scanShort')}</Text>
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
              title={pair('switchToCode')}
              variant="text"
              fullWidth
              onPress={() => {
                setScanOpen(false);
                flow.openCodeSheet('enter');
              }}
            />
          </AppHost>
        </View>
      </AppBottomSheet>

      {/* Suggested Device Sheet */}
      <AppBottomSheet
        visible={selectedCandidate !== null}
        onDismiss={() => setSelectedCandidate(null)}
      >
        {selectedCandidate ? (
          <View style={styles.sheetBody}>
            <View style={styles.detailHeader}>
              <View style={[styles.detailIcon, { backgroundColor: c.accentContainer }]}>
                <Laptop size={22} color={c.onAccentContainer as string} />
              </View>
              <View style={styles.flex1}>
                <Text style={[styles.sheetTitle, { color: c.textPrimary }]} numberOfLines={1}>
                  {selectedCandidate.name || selectedCandidate.device_id.slice(0, 8)}
                </Text>
                <Text style={[styles.detailSub, { color: c.textSecondary }]}>
                  {selectedCandidate.introducers?.[0]?.name
                    ? pair('recommendedBy', { name: selectedCandidate.introducers[0].name })
                    : pair('recommendedHint')}
                </Text>
              </View>
            </View>
            <AppHost matchContents={{ vertical: true }}>
              <AppButton
                title={pair('connect')}
                variant="filled"
                size="large"
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

      {/* Polished M3 Device Detail Sheet */}
      <AppBottomSheet visible={trusted !== undefined} onDismiss={() => setSelectedDeviceID(null)}>
        {trusted ? (
          <View style={styles.sheetBody}>
            <View style={styles.detailHeader}>
              <View style={[styles.detailIcon, { backgroundColor: c.accentContainer }]}>
                <Laptop size={22} color={c.onAccentContainer as string} />
              </View>
              <View style={styles.flex1}>
                <Text style={[styles.sheetTitle, { color: c.textPrimary }]} numberOfLines={1}>
                  {deviceDisplayName(trusted)}
                </Text>
                <Text style={[styles.detailSub, { color: c.textSecondary }]}>
                  {pair(deviceStatusKind(trusted))}
                </Text>
              </View>
            </View>

            {trusted.bindings?.map((binding) => (
              <View key={binding.id} style={[styles.detailCardRow, { backgroundColor: c.surfaceMid }]}>
                <View style={styles.flex1}>
                  <Text style={[styles.rowTitle, { color: c.textPrimary }]}>
                    {pair('autoSyncToggle')}
                  </Text>
                  <Text style={[styles.rowSub, { color: c.textSecondary }]}>
                    {pair(binding.sync_enabled ? 'autoSyncOnHint' : 'autoSyncOffHint')}
                  </Text>
                </View>
                <AppHost matchContents>
                  <AppSwitch
                    value={binding.sync_enabled}
                    onValueChange={(enabled: boolean) =>
                      void run(() => store.setBindingSync(binding.id, binding.revision, enabled))
                    }
                  />
                </AppHost>
              </View>
            ))}

            <View style={[styles.detailCard, { backgroundColor: c.surfaceMid }]}>
              <Text style={[styles.fieldLabel, { color: c.textSecondary }]}>{pair('rename')}</Text>
              <TextInput
                testID="devices-rename-input"
                accessibilityLabel={pair('rename')}
                value={name}
                onChangeText={setName}
                style={[styles.renameInput, { backgroundColor: c.surfaceLow, color: c.textPrimary }]}
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
            </View>

            <View style={[styles.detailCard, { backgroundColor: c.surfaceMid }]}>
              <View style={styles.fpLabelRow}>
                <ShieldCheck size={14} color={c.accent as string} />
                <Text style={[styles.fieldLabel, { color: c.textSecondary }]}>
                  {pair('fingerprintLabel')}
                </Text>
              </View>
              <Text selectable style={[styles.fpText, { color: c.textPrimary }]}>
                {formatFingerprintLines(trusted.fingerprint)}
              </Text>
            </View>

            <AppHost matchContents={{ vertical: true }}>
              <AppButton
                title={pair('unpair')}
                variant="tonal"
                fullWidth
                colors={{ containerColor: c.errorContainer, contentColor: c.error }}
                onPress={() => {
                  const target = trusted;
                  setSelectedDeviceID(null);
                  setConfirmUnpairDevice(target);
                }}
              />
            </AppHost>
          </View>
        ) : null}
      </AppBottomSheet>

      <AppAlertDialog
        visible={confirmUnpairDevice !== null}
        standalone
        title={pair('unpair')}
        message={
          confirmUnpairDevice
            ? pair('unpairConfirmNamed', { device: deviceDisplayName(confirmUnpairDevice) })
            : ''
        }
        confirmLabel={pair('unpair')}
        onConfirm={() => {
          const target = confirmUnpairDevice;
          setConfirmUnpairDevice(null);
          if (target) void run(() => store.unpairDevice(target.id, target.revision));
        }}
        dismissLabel={t('common:action.cancel')}
        onDismiss={() => setConfirmUnpairDevice(null)}
        onDismissAction={() => setConfirmUnpairDevice(null)}
      />
    </SafeAreaView>
  );
}

const styles = StyleSheet.create({
  container: { flex: 1 },
  flex1: { flex: 1 },
  sheetBody: {
    paddingHorizontal: 20,
    paddingBottom: 20,
    gap: 14,
  },
  sheetTitle: {
    fontSize: 20,
    fontWeight: '700',
  },
  detailHeader: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 14,
  },
  detailIcon: {
    width: 46,
    height: 46,
    borderRadius: 14,
    alignItems: 'center',
    justifyContent: 'center',
  },
  detailSub: {
    fontSize: 13,
    marginTop: 2,
  },
  detailCardRow: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: 12,
    borderRadius: 18,
    padding: 16,
  },
  detailCard: {
    borderRadius: 18,
    padding: 16,
    gap: 10,
  },
  rowTitle: {
    fontSize: 15,
    fontWeight: '600',
  },
  rowSub: {
    fontSize: 13,
    marginTop: 2,
  },
  fieldLabel: {
    fontSize: 12,
    fontWeight: '600',
  },
  renameInput: {
    borderRadius: 12,
    paddingHorizontal: 14,
    paddingVertical: 11,
    fontSize: 15,
  },
  fpLabelRow: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 6,
  },
  fpText: {
    fontSize: 15,
    lineHeight: 24,
    fontWeight: '600',
    letterSpacing: 1.2,
    textAlign: 'center',
    fontVariant: ['tabular-nums'],
  },
});
