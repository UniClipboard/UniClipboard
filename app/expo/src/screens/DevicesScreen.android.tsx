/**
 * 设备管理页(Android) — 对齐原仓库 UniClip M3 Expressive 设计（UnifiedSpaceSetup.android.tsx），
 * 接入 v2 Go Engine 设备身份、配对码/票据/扫码与指纹批准流程。
 *
 * 布局: 顶部 28dp 状态概览卡（44dp 状态徽标 + 摘要 + 主/次操作按钮）
 *      → 本机（带复制设备 ID）→ 进行中的配对 → 待批准设备 → 已配对设备 → 配对新设备 → 推荐设备。
 * 刷新动作挂在顶级「设备」页面标题栏右侧。
 */
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { Alert, AppState, Image, StyleSheet, Text, TextInput, View } from 'react-native';
import { useIsFocused, useNavigation } from '@react-navigation/native';
import type { NativeStackNavigationProp } from '@react-navigation/native-stack';
import {
  Box,
  Button,
  CircularProgressIndicator,
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
import { SafeAreaView, useSafeAreaInsets } from 'react-native-safe-area-context';
import { useTranslation } from 'react-i18next';

import { useFloatingNavigationClearance } from '@/components/android/floatingNavigationClearance';
import { M3IconButton } from '@/components/android/M3IconButton';
import { AppAlertDialog, AppBottomSheet, AppButton, AppHost } from '@/components/ui';
import { parseEngineError, unicEngineNative } from '@/engine/nativeModule';
import { useEngineStore } from '@/engine/store';
import type {
  EngineCandidate,
  EngineDevice,
  EngineEvent,
  EngineOperation,
  SensitiveTicket,
} from '@/engine/types';
import { useTheme } from '@/hooks/useTheme';
import type { RootStackParamList, SpaceDeviceTarget } from '@/navigation/AppNavigator.types';
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
const CODE_STYLE = { fontSize: 24, fontWeight: '700', letterSpacing: 2 } as const;
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
  close: require('../assets/icons/close.xml'),
  copy: require('../assets/icons/content_copy.xml'),
  device: require('../assets/icons/devices.xml'),
  join: require('../assets/icons/account_circle.xml'),
  phone: require('../assets/icons/smartphone.xml'),
  qr: require('../assets/icons/qr_code_scanner.xml'),
  share: require('../assets/icons/share.xml'),
  status: require('../assets/icons/circle.xml'),
};

function deviceName(device: EngineDevice) {
  return device.name || device.reported_name || device.id.slice(0, 8);
}

function deviceStatusKey(device: EngineDevice) {
  const binding = device.bindings?.[0];
  if (!binding) return 'noBinding';
  if (binding.connection === 'connected') return binding.sync_enabled ? 'connected' : 'paused';
  if (binding.connection === 'connecting') return 'connecting';
  return 'offline';
}

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
  const binding = device.bindings?.[0];
  const connected = binding?.connection === 'connected' && binding.sync_enabled;
  const paused = binding?.connection === 'connected' && !binding.sync_enabled;
  const connecting = binding?.connection === 'connecting';
  const statusColor = connected
    ? (theme.colors.success as string)
    : paused || connecting
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
        <ComposeText>{deviceName(device)}</ComposeText>
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

function PlaceholderRow({ id, label }: { id: string; label: string }) {
  const colors = useMaterialColors();
  const rowColors = useSettingsSectionRowColors();
  return (
    <ListItem colors={rowColors} modifiers={[testID(id)]}>
      <ListItem.LeadingContent>
        <SettingsLeadingIcon source={ICONS.device} tone="muted" />
      </ListItem.LeadingContent>
      <ListItem.HeadlineContent>
        <ComposeText color={colors.onSurfaceVariant}>{label}</ComposeText>
      </ListItem.HeadlineContent>
    </ListItem>
  );
}

function DevicesOverviewHero({
  pendingCount,
  onlineCount,
  offlineCount,
  pairingCode,
  onCreateCode,
  onCancelCode,
  onOpenScan,
}: {
  pendingCount: number;
  onlineCount: number;
  offlineCount: number;
  pairingCode: string | null;
  onCreateCode: () => void;
  onCancelCode: () => void;
  onOpenScan: () => void;
}) {
  const { t } = useTranslation();
  const pair = (key: string, values?: Record<string, string>) =>
    t(`settingsSync:pairing.${key}`, values);
  const colors = useMaterialColors();
  const hasPending = pendingCount > 0;
  const hasTrusted = onlineCount + offlineCount > 0;
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
    ? pair('pending')
    : healthy
      ? t('settingsSync:space.overview.status.healthy')
      : hasTrusted
        ? t('settingsSync:space.overview.noDevicesOnline')
        : t('settingsSync:space.overview.noOtherDevices');

  const overviewBody = hasTrusted
    ? t('settingsSync:space.overview.deviceSummary', {
        online: onlineCount,
        offline: offlineCount,
      })
    : pair('instructions');

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

        {pairingCode ? (
          <>
            <Spacer modifiers={[heightModifier(14)]} />
            <Surface
              color={colors.surfaceContainerLowest}
              shape={Shape.RoundedCorner({
                cornerRadii: { topStart: 16, topEnd: 16, bottomStart: 16, bottomEnd: 16 },
              })}
              modifiers={[fillMaxWidth()]}
            >
              <Row
                verticalAlignment="center"
                modifiers={[fillMaxWidth(), padding(16, 12, 16, 12)]}
              >
                <Column modifiers={[weight(1)]}>
                  <ComposeText color={colors.primary} style={CODE_STYLE}>
                    {pairingCode}
                  </ComposeText>
                  <ComposeText color={colors.onSurfaceVariant}>
                    {pair('codeInstructions')}
                  </ComposeText>
                </Column>
                <FilledTonalButton
                  modifiers={[testID('devices-cancel-code')]}
                  onClick={onCancelCode}
                >
                  <ComposeText>{t('common:action.cancel')}</ComposeText>
                </FilledTonalButton>
              </Row>
            </Surface>
          </>
        ) : null}

        <Spacer modifiers={[heightModifier(16)]} />
        <Button
          modifiers={[fillMaxWidth(), testID('devices-create-code')]}
          onClick={onCreateCode}
        >
          <Icon source={ICONS.add} size={18} tint={colors.onPrimary} />
          <Spacer modifiers={[widthModifier(8)]} />
          <ComposeText maxLines={1}>{pair('generateCode')}</ComposeText>
        </Button>
        <Spacer modifiers={[heightModifier(8)]} />
        <FilledTonalButton
          modifiers={[fillMaxWidth(), testID('devices-scan')]}
          onClick={onOpenScan}
        >
          <Icon source={ICONS.qr} size={18} tint={colors.onSecondaryContainer} />
          <Spacer modifiers={[widthModifier(8)]} />
          <ComposeText maxLines={1}>{pair('scan')}</ComposeText>
        </FilledTonalButton>
      </Column>
    </Surface>
  );
}

export function DevicesScreen(_props: SpaceDeviceTarget) {
  const { t } = useTranslation();
  const pair = (key: string, values?: Record<string, string>) =>
    t(`settingsSync:pairing.${key}`, values);
  const { theme } = useTheme();
  const insets = useSafeAreaInsets();
  const navClearance = useFloatingNavigationClearance();
  const navigation = useNavigation<NativeStackNavigationProp<RootStackParamList>>();
  const focused = useIsFocused();

  const mode = useEngineStore((s) => s.mode);
  const self = useEngineStore((s) => s.snapshot?.self);
  const devices = useEngineStore((s) => s.devices);
  const candidates = useEngineStore((s) => s.candidates);
  const pairingCode = useEngineStore((s) => s.pairingCode);

  const [joinMode, setJoinMode] = useState<'manual' | 'scan' | null>(null);
  const [joinInput, setJoinInput] = useState('');
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
  const [name, setName] = useState('');
  const [foreground, setForeground] = useState(AppState.currentState === 'active');

  const closeTicket = useCallback(() => {
    ticketGeneration.current++;
    setTicket(null);
  }, []);

  const errorText = useCallback(
    (error: unknown) => {
      const code = parseEngineError(error).code;
      return t(`errors:engine.${code}`, { defaultValue: t('errors:engine.internal') });
    },
    [t]
  );

  const run = useCallback(
    async (action: () => Promise<unknown>) => {
      try {
        await action();
      } catch (error) {
        Alert.alert(pair('actionFailed'), errorText(error));
      }
    },
    [errorText, t]
  );

  // 刷新放在顶级「设备」目的地标题栏右侧，与原仓库 UnifiedSpaceSetup.android.tsx 一致
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
    const subscription = AppState.addEventListener('change', (state) => {
      const active = state === 'active';
      setForeground(active);
      if (!active) closeTicket();
    });
    return () => subscription.remove();
  }, [closeTicket]);

  useEffect(() => {
    if (focused && mode === 'live') return;
    setJoinMode(null);
    setJoinInput('');
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
    setJoinMode(null);
    setJoinInput('');
    if (/^\d{3}-?\d{3}$/.test(digits)) {
      void start(() => store.joinByCode(digits));
    } else if (raw.startsWith('unic-pair:')) {
      void start(() => store.joinByTicket(raw));
    } else {
      Alert.alert(pair('actionFailed'), pair('invalidInput'));
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

  const contentPadding = {
    start: 16,
    end: 16,
    top: 8,
    bottom: insets.bottom + navClearance + 40,
  };

  if (mode !== 'live') {
    return (
      <SafeAreaView
        style={[styles.container, { backgroundColor: theme.colors.background }]}
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
  const pendingDevices = devices.filter((d) => d.trust === 'pending');
  const trustedDevices = [...devices.filter((d) => d.trust === 'trusted')].sort((left, right) => {
    const rank = (d: EngineDevice) => (d.bindings?.[0]?.connection === 'connected' ? 0 : 1);
    return rank(left) - rank(right);
  });
  const onlineCount = trustedDevices.filter(
    (d) => d.bindings?.[0]?.connection === 'connected'
  ).length;
  const offlineCount = trustedDevices.length - onlineCount;
  const trusted = trustedDevices.find((d) => d.id === selectedDeviceID);
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

  const titleStyle = { color: theme.colors.textPrimary, fontSize: 18, fontWeight: '700' as const };
  const inputStyle = {
    borderRadius: 12,
    padding: 12,
    backgroundColor: theme.colors.surface,
    color: theme.colors.textPrimary,
  };
  const closeJoin = () => {
    setJoinMode(null);
    setJoinInput('');
  };

  return (
    <SafeAreaView
      style={[styles.container, { backgroundColor: theme.colors.background }]}
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
            pairingCode={pairingCode?.code ?? null}
            onCreateCode={() => void run(store.createPairingCode)}
            onCancelCode={() => void run(store.cancelPairingCode)}
            onOpenScan={() => setJoinMode('scan')}
          />

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

          {starting || operationID ? (
            <SettingsSectionItem variant="grouped" title={pair('progress')}>
              <ListItem modifiers={[testID('devices-pairing-status')]}>
                <ListItem.LeadingContent>
                  <CircularProgressIndicator
                    modifiers={[widthModifier(24), heightModifier(24)]}
                  />
                </ListItem.LeadingContent>
                <ListItem.HeadlineContent>
                  <ComposeText>{operationStatus}</ComposeText>
                </ListItem.HeadlineContent>
              </ListItem>
              <SettingsListRow
                key="stop-waiting"
                testID="devices-stop-waiting"
                icon={ICONS.close}
                iconTone="muted"
                title={pair('stopWaiting')}
                description={pair('stopWaitingHelp')}
                onPress={() => {
                  operationGeneration.current++;
                  setStarting(false);
                  setOperationID(null);
                  setOperation(null);
                  setOperationError(null);
                }}
              />
            </SettingsSectionItem>
          ) : null}

          {pendingDevices.length > 0 ? (
            <SettingsSectionItem variant="grouped" title={pair('pending')}>
              {pendingDevices.map((device) => (
                <SettingsListRow
                  key={device.id}
                  testID={`pending-${device.id}`}
                  icon={ICONS.alert}
                  tone="warning"
                  title={deviceName(device)}
                  description={pair('fingerprint', { value: device.fingerprint })}
                  trailing={{ pill: pair('review') }}
                  onPress={() => setPendingApproveDevice(device)}
                />
              ))}
            </SettingsSectionItem>
          ) : null}

          <SettingsSectionItem variant="grouped" title={pair('trusted')}>
            {trustedDevices.length > 0 ? (
              trustedDevices.map((device) => (
                <TrustedDeviceRow
                  key={device.id}
                  device={device}
                  statusText={pair(deviceStatusKey(device))}
                  onSelect={() => {
                    setName(device.name || device.reported_name || '');
                    setSelectedDeviceID(device.id);
                  }}
                />
              ))
            ) : (
              <PlaceholderRow id="device-empty" label={pair('empty')} />
            )}
          </SettingsSectionItem>

          <SettingsSectionItem
            variant="grouped"
            title={pair('newDevice')}
            footer={pairingCode ? pair('codeInstructions') : pair('instructions')}
          >
            <SettingsListRow
              key="join"
              testID="devices-join"
              icon={ICONS.join}
              title={pair('enterCode')}
              trailing="chevron"
              onPress={() => setJoinMode('manual')}
            />
            <SettingsListRow
              key="export-ticket"
              testID="devices-export-ticket"
              icon={ICONS.share}
              title={pair('showTicket')}
              trailing="chevron"
              onPress={() => void exportTicket()}
            />
          </SettingsSectionItem>

          {candidates.length > 0 ? (
            <SettingsSectionItem variant="grouped" title={pair('recommended')}>
              {candidates.map((candidate) => (
                <SettingsListRow
                  key={candidate.device_id}
                  testID={`candidate-${candidate.device_id}`}
                  icon={ICONS.device}
                  iconTone="muted"
                  title={candidate.name || candidate.device_id.slice(0, 8)}
                  trailing={{
                    text: pair(candidate.requested ? 'progress' : 'review'),
                    chevron: true,
                  }}
                  onPress={() => setSelectedCandidate(candidate)}
                />
              ))}
            </SettingsSectionItem>
          ) : null}
        </LazyColumn>
      </Host>

      <AppAlertDialog
        visible={pendingApproveDevice !== null}
        standalone
        title={pair('approveTitle')}
        message={
          pendingApproveDevice
            ? pair('approveBody', {
                name: deviceName(pendingApproveDevice),
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

      <AppBottomSheet visible={joinMode !== null} onDismiss={closeJoin}>
        <View style={{ padding: 16, gap: 12 }}>
          <Text style={titleStyle}>{pair(joinMode === 'scan' ? 'scan' : 'enterCode')}</Text>
          {joinMode === 'scan' ? (
            focused && foreground && <PairingScanner onRead={submit} />
          ) : (
            <>
              <TextInput
                testID="devices-join-input"
                value={joinInput}
                onChangeText={setJoinInput}
                placeholder={pair('inputHint')}
                placeholderTextColor={theme.colors.textSecondary}
                autoCapitalize="none"
                autoCorrect={false}
                style={inputStyle}
              />
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="devices-join-submit"
                  title={pair('connect')}
                  variant="filled"
                  fullWidth
                  disabled={starting || !joinInput.trim()}
                  onPress={() => submit(joinInput)}
                />
              </AppHost>
            </>
          )}
          <AppHost matchContents={{ vertical: true }}>
            <AppButton
              title={t('common:action.cancel')}
              variant="text"
              fullWidth
              onPress={closeJoin}
            />
          </AppHost>
        </View>
      </AppBottomSheet>

      <AppBottomSheet visible={ticket !== null && focused && foreground} onDismiss={closeTicket}>
        {ticket && focused && foreground && (
          <View style={{ padding: 16, gap: 12 }}>
            <Text style={titleStyle}>{pair('ticketTitle')}</Text>
            <Text style={{ color: theme.colors.textPrimary }}>{pair('ticketHelp')}</Text>
            {ticket.qr_png ? (
              <Image
                testID="devices-pairing-qr"
                accessibilityLabel={pair('ticketTitle')}
                source={{ uri: `data:image/png;base64,${ticket.qr_png}` }}
                style={{ width: 260, height: 260, alignSelf: 'center' }}
              />
            ) : null}
            <AppHost matchContents={{ vertical: true }}>
              <AppButton
                title={t('common:action.copy')}
                variant="filled"
                fullWidth
                onPress={() =>
                  void run(async () => {
                    if (ticket.expires_at_ms <= Date.now()) {
                      closeTicket();
                      return;
                    }
                    await unicEngineNative.copyPairingTicket(ticket.ticket, ticket.expires_at_ms);
                    Alert.alert(t('errors:copy.copied'));
                  })
                }
              />
            </AppHost>
            <AppHost matchContents={{ vertical: true }}>
              <AppButton
                title={t('common:action.close')}
                variant="text"
                fullWidth
                onPress={closeTicket}
              />
            </AppHost>
          </View>
        )}
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
                disabled={starting || selectedCandidate.requested}
                onPress={() => {
                  setSelectedCandidate(null);
                  void start(() => store.pairCandidate(selectedCandidate));
                }}
              />
            </AppHost>
            <AppHost matchContents={{ vertical: true }}>
              <AppButton
                title={pair('ignore')}
                variant="outlined"
                fullWidth
                onPress={() => {
                  setSelectedCandidate(null);
                  void run(() => store.ignoreCandidate(selectedCandidate.device_id, true));
                }}
              />
            </AppHost>
          </View>
        ) : null}
      </AppBottomSheet>

      <AppBottomSheet visible={trusted !== undefined} onDismiss={() => setSelectedDeviceID(null)}>
        {trusted ? (
          <View style={{ padding: 16, gap: 12 }}>
            <Text style={titleStyle}>{deviceName(trusted)}</Text>
            <Text style={{ color: theme.colors.textSecondary }}>
              {pair('fingerprint', { value: trusted.fingerprint })}
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
                title={pair('rename')}
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
                      store.setBindingSync(binding.id, binding.revision, !binding.sync_enabled)
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
    </SafeAreaView>
  );
}

const styles = StyleSheet.create({
  container: { flex: 1 },
});
