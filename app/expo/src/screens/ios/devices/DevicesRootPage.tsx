import {
  Button as SwiftUIButton,
  HStack,
  Image,
  ProgressView,
  Section,
  Spacer,
  Text as SwiftUIText,
  VStack,
} from '@expo/ui/swift-ui';
import {
  buttonBorderShape,
  buttonStyle,
  contentShape,
  controlSize,
  cornerRadius,
  background,
  font,
  foregroundStyle,
  frame,
  padding,
  shapes,
} from '@expo/ui/swift-ui/modifiers';
import { useTranslation } from 'react-i18next';

import { IosSheetForm, IosSheetPage } from '@/components/ui';
import { iosProminentButtonModifiers } from '@/components/ui/iosButtonStyles.ios';
import type { EngineCandidate, EngineDevice, PairingCode } from '@/engine/types';
import {
  SettingsNavRow,
  chevronColor,
  settingsTileColors,
  statusGreen,
} from '@/screens/settings/ios/common';
import {
  DeviceIconTile,
  EngineDeviceRow,
  LocalDeviceRow,
  OFFLINE_DOT,
  engineDeviceStatus,
} from './deviceRows';

const capsuleButton = (prominent: boolean, size: 'regular' | 'large' = 'regular') => [
  ...(prominent ? iosProminentButtonModifiers() : [buttonStyle('bordered')]),
  buttonBorderShape('capsule'),
  controlSize(size),
];

export interface DevicesRootPageProps {
  mode: 'live' | 'unavailable';
  self?: { id: string; name: string };
  devices: EngineDevice[];
  candidates: EngineCandidate[];
  pairingCode: PairingCode | null;
  starting: boolean;
  operationID: string | null;
  operationStatus: string;
  onRefresh: () => void;
  onCopyLocalID: () => void;
  onCreatePairingCode: () => void;
  onCancelPairingCode: () => void;
  onOpenManualJoin: () => void;
  onOpenScanJoin: () => void;
  onExportTicket: () => void;
  onStopWaiting: () => void;
  onApprovePending: (device: EngineDevice) => void;
  onSelectDevice: (device: EngineDevice) => void;
  onSelectCandidate: (candidate: EngineCandidate) => void;
  onOpenConnectionPreview: () => void;
  onShowUnavailable: () => void;
}

/**
 * iOS「设备」标签页根页（对齐 v1 DevicesRootPage.tsx）：
 * 顶部状态总览卡（44pt 圆形徽标 + 状态摘要 + 胶囊快捷按钮）→ 本机 → 待批准设备 → 已配对设备 → 配对新设备 → 推荐设备。
 */
export function DevicesRootPage({
  mode,
  self,
  devices,
  candidates,
  pairingCode,
  starting,
  operationID,
  operationStatus,
  onRefresh,
  onCopyLocalID,
  onCreatePairingCode,
  onCancelPairingCode,
  onOpenManualJoin,
  onOpenScanJoin,
  onExportTicket,
  onStopWaiting,
  onApprovePending,
  onSelectDevice,
  onSelectCandidate,
  onOpenConnectionPreview,
  onShowUnavailable,
}: DevicesRootPageProps) {
  const { t } = useTranslation();
  const pair = (key: string, values?: Record<string, string>) =>
    t(`settingsSync:pairing.${key}`, values);

  if (mode !== 'live') {
    return (
      <IosSheetPage title={t('home:nav.devices')}>
        <IosSheetForm>
          <Section
            header={<SwiftUIText>{t('settings:syncChannel.title')}</SwiftUIText>}
            footer={<SwiftUIText>{t('shell:unavailable')}</SwiftUIText>}
          >
            <SettingsNavRow
              testID="devices-add"
              icon="plus.circle.fill"
              iconColor={settingsTileColors.blue}
              title={t('shell:addConnection')}
              onPress={onOpenConnectionPreview}
            />
          </Section>
          <Section header={<SwiftUIText>{t('shell:demoDevices')}</SwiftUIText>}>
            {(['demoMac', 'demoPhone'] as const).map((device, index) => (
              <SwiftUIButton
                key={device}
                testID={`device-${index}`}
                onPress={onShowUnavailable}
                modifiers={[buttonStyle('plain')]}
              >
                <HStack
                  spacing={16}
                  alignment="center"
                  modifiers={[
                    frame({ maxWidth: Infinity, minHeight: 48 }),
                    contentShape(shapes.rectangle()),
                  ]}
                >
                  <DeviceIconTile systemName={device === 'demoMac' ? 'laptopcomputer' : 'iphone'} />
                  <VStack alignment="leading" spacing={1}>
                    <SwiftUIText>{pair(device)}</SwiftUIText>
                    <SwiftUIText modifiers={[font({ size: 13 }), foregroundStyle('secondary')]}>
                      {t('shell:disconnected')}
                    </SwiftUIText>
                  </VStack>
                  <Spacer />
                  <Image systemName="circle.fill" size={8} color={OFFLINE_DOT} />
                  <Image systemName="chevron.right" size={13} color={chevronColor} />
                </HStack>
              </SwiftUIButton>
            ))}
          </Section>
        </IosSheetForm>
      </IosSheetPage>
    );
  }

  const pendingDevices = devices.filter((d) => d.trust === 'pending');
  const trustedDevices = [...devices.filter((d) => d.trust === 'trusted')].sort((left, right) => {
    const rank = (d: EngineDevice) => (d.bindings?.[0]?.connection === 'connected' ? 0 : 1);
    return rank(left) - rank(right);
  });
  const onlineCount = trustedDevices.filter(
    (d) => d.bindings?.[0]?.connection === 'connected'
  ).length;
  const offlineCount = trustedDevices.length - onlineCount;

  const hasPending = pendingDevices.length > 0;
  const hasTrusted = trustedDevices.length > 0;
  const badgeColor = hasPending
    ? settingsTileColors.orange
    : onlineCount > 0
      ? statusGreen
      : settingsTileColors.blue;
  const badgeIcon = hasPending
    ? 'exclamationmark'
    : onlineCount > 0
      ? 'checkmark'
      : 'laptopcomputer.and.iphone';
  const overviewTitle = hasPending
    ? pair('pending')
    : onlineCount > 0
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

  const refreshButton = (
    <SwiftUIButton
      key="refresh"
      testID="devices-refresh"
      systemImage="arrow.clockwise"
      label={t('common:action.refresh')}
      onPress={onRefresh}
    />
  );

  return (
    <IosSheetPage title={t('home:nav.devices')} rightSlots={[refreshButton]}>
      <IosSheetForm>
        {/* ── 状态总览卡（对齐 v1 DevicesRootPage 状态卡） ── */}
        <Section>
          <VStack spacing={14} alignment="leading" modifiers={[padding({ vertical: 6 })]}>
            <HStack
              spacing={14}
              alignment="top"
              modifiers={[frame({ maxWidth: Infinity, alignment: 'leading' })]}
            >
              {starting || operationID ? (
                <ProgressView modifiers={[frame({ width: 44, height: 44 })]} />
              ) : (
                <Image
                  systemName={badgeIcon}
                  size={20}
                  color="white"
                  modifiers={[
                    frame({ width: 44, height: 44 }),
                    background(badgeColor),
                    cornerRadius(22),
                  ]}
                />
              )}
              <VStack alignment="leading" spacing={3}>
                <SwiftUIText modifiers={[font({ size: 17, weight: 'semibold' })]}>
                  {starting || operationID ? operationStatus : overviewTitle}
                </SwiftUIText>
                <SwiftUIText modifiers={[font({ size: 15 }), foregroundStyle('secondary')]}>
                  {overviewBody}
                </SwiftUIText>
              </VStack>
              <Spacer />
            </HStack>

            <HStack spacing={10} modifiers={[padding({ leading: 58 })]}>
              <SwiftUIButton
                testID="devices-join"
                onPress={onOpenManualJoin}
                modifiers={capsuleButton(false)}
              >
                <HStack spacing={6}>
                  <Image systemName="plus" size={14} />
                  <SwiftUIText modifiers={[font({ weight: 'semibold' })]}>
                    {pair('enterCode')}
                  </SwiftUIText>
                </HStack>
              </SwiftUIButton>
            </HStack>
          </VStack>
        </Section>

        {/* ── 配对进度（仅配对进行时显示） ── */}
        {starting || operationID ? (
          <Section header={<SwiftUIText>{pair('progress')}</SwiftUIText>}>
            <SettingsNavRow
              testID="devices-pairing-status"
              icon="dot.radiowaves.left.and.right"
              iconColor={settingsTileColors.blue}
              title={operationStatus}
              readOnly
            />
            <SettingsNavRow
              testID="devices-stop-waiting"
              icon="xmark.circle"
              iconColor={settingsTileColors.orange}
              title={pair('stopWaiting')}
              subtitle={pair('stopWaitingHelp')}
              showsChevron={false}
              onPress={onStopWaiting}
            />
          </Section>
        ) : null}

        {/* ── 本机（对齐 v1 thisDevice 分组，支持等宽 ID 点按复制） ── */}
        {self ? (
          <Section header={<SwiftUIText>{pair('thisDevice')}</SwiftUIText>}>
            <LocalDeviceRow
              name={self.name}
              deviceID={self.id}
              subtitle={pair('deviceID', { id: self.id })}
              onCopyID={onCopyLocalID}
            />
          </Section>
        ) : null}

        {/* ── 待批准设备（强制指纹核对） ── */}
        {pendingDevices.length > 0 ? (
          <Section
            header={<SwiftUIText>{`${pair('pending')} · ${pendingDevices.length}`}</SwiftUIText>}
          >
            {pendingDevices.map((device) => {
              const status = engineDeviceStatus(device, pair);
              return (
                <EngineDeviceRow
                  key={device.id}
                  testID={`pending-${device.id}`}
                  device={device}
                  subtitle={pair('fingerprint', { value: device.fingerprint })}
                  statusLabel={status.label}
                  dotColor={status.dot}
                  onPress={() => onApprovePending(device)}
                />
              );
            })}
          </Section>
        ) : null}

        {/* ── 已配对设备（有设备时展示；0 台时避免与顶部状态卡重复空态并消除底栏遮挡） ── */}
        {trustedDevices.length > 0 ? (
          <Section
            header={<SwiftUIText>{`${pair('trusted')} · ${trustedDevices.length}`}</SwiftUIText>}
          >
            {trustedDevices.map((device) => {
              const status = engineDeviceStatus(device, pair);
              return (
                <EngineDeviceRow
                  key={device.id}
                  testID={`device-${device.id}`}
                  device={device}
                  statusLabel={status.label}
                  dotColor={status.dot}
                  onPress={() => onSelectDevice(device)}
                />
              );
            })}
          </Section>
        ) : null}

        {/* ── 配对新设备（带真实 SF Symbol 图标） ── */}
        <Section
          header={<SwiftUIText>{pair('newDevice')}</SwiftUIText>}
          footer={
            <SwiftUIText>
              {pairingCode ? pair('codeInstructions') : pair('instructions')}
            </SwiftUIText>
          }
        >
          <SettingsNavRow
            testID="devices-create-code"
            icon="number.circle.fill"
            iconColor={pairingCode ? settingsTileColors.green : settingsTileColors.blue}
            title={pairingCode ? pair('code', { code: pairingCode.code }) : pair('generateCode')}
            onPress={onCreatePairingCode}
          />
          {pairingCode ? (
            <SettingsNavRow
              testID="devices-cancel-code"
              icon="xmark.circle"
              iconColor={settingsTileColors.red}
              title={pair('cancelCode')}
              destructive
              showsChevron={false}
              onPress={onCancelPairingCode}
            />
          ) : null}
          <SettingsNavRow
            testID="devices-scan"
            icon="qrcode.viewfinder"
            iconColor={settingsTileColors.indigo}
            title={pair('scan')}
            onPress={onOpenScanJoin}
          />
          <SettingsNavRow
            testID="devices-export-ticket"
            icon="qrcode"
            iconColor={settingsTileColors.purple}
            title={pair('showTicket')}
            onPress={onExportTicket}
          />
        </Section>

        {/* ── 推荐设备 ── */}
        {candidates.length > 0 ? (
          <Section
            header={<SwiftUIText>{`${pair('recommended')} · ${candidates.length}`}</SwiftUIText>}
          >
            {candidates.map((candidate) => (
              <SwiftUIButton
                key={candidate.device_id}
                testID={`candidate-${candidate.device_id}`}
                onPress={() => onSelectCandidate(candidate)}
                modifiers={[buttonStyle('plain')]}
              >
                <HStack
                  spacing={16}
                  alignment="center"
                  modifiers={[
                    frame({ maxWidth: Infinity, minHeight: 48 }),
                    contentShape(shapes.rectangle()),
                  ]}
                >
                  <DeviceIconTile systemName="laptopcomputer" />
                  <VStack alignment="leading" spacing={1}>
                    <SwiftUIText>{candidate.name || candidate.device_id.slice(0, 8)}</SwiftUIText>
                    <SwiftUIText modifiers={[font({ size: 13 }), foregroundStyle('secondary')]}>
                      {pair(candidate.requested ? 'progress' : 'review')}
                    </SwiftUIText>
                  </VStack>
                  <Spacer />
                  <Image systemName="chevron.right" size={13} color={chevronColor} />
                </HStack>
              </SwiftUIButton>
            ))}
          </Section>
        ) : null}
      </IosSheetForm>
    </IosSheetPage>
  );
}
