import {
  Button as SwiftUIButton,
  HStack,
  Image,
  Section,
  Spacer,
  Text as SwiftUIText,
  VStack,
} from '@expo/ui/swift-ui';
import {
  background,
  buttonBorderShape,
  buttonStyle,
  contentShape,
  controlSize,
  cornerRadius,
  font,
  foregroundStyle,
  frame,
  padding,
  shapes,
} from '@expo/ui/swift-ui/modifiers';
import { PlatformColor } from 'react-native';
import { useTranslation } from 'react-i18next';

import { IosSheetForm, IosSheetPage } from '@/components/ui';
import { iosProminentButtonModifiers } from '@/components/ui/iosButtonStyles.ios';
import type { EngineCandidate, EngineDevice, PairingCode } from '@/engine/types';
import {
  deviceStatusKind,
  formatFingerprintShort,
  formatPairingCodeDisplay,
  needsDeviceApproval,
  sortTrustedDevices,
} from '@/screens/pairing/pairingUtils';
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
  pairingCodeCountdown: string;
  onRefresh: () => void;
  onCopyLocalID: () => void;
  onOpenScanJoin: () => void;
  onOpenCodeSheet: (initialTab?: 'enter' | 'show') => void;
  onReviewPending: (device: EngineDevice) => void;
  onSelectDevice: (device: EngineDevice) => void;
  onSelectCandidate: (candidate: EngineCandidate) => void;
  onOpenConnectionPreview: () => void;
  onShowUnavailable: () => void;
}

/**
 * Simplified iOS "Devices" tab root page:
 * 1. Overview Hero Card (plain status + Scan QR & Pairing Code buttons + active code pill)
 * 2. Pending Approval section (only when a dismissed approval is still awaiting decision)
 * 3. Paired Devices list (online first, clean status labels)
 * 4. Suggested Devices (only when introduced candidates exist)
 * 5. Compact "This Device" section at bottom (tap to copy ID)
 */
export function DevicesRootPage({
  mode,
  self,
  devices,
  candidates,
  pairingCode,
  pairingCodeCountdown,
  onRefresh,
  onCopyLocalID,
  onOpenScanJoin,
  onOpenCodeSheet,
  onReviewPending,
  onSelectDevice,
  onSelectCandidate,
  onOpenConnectionPreview,
  onShowUnavailable,
}: DevicesRootPageProps) {
  const { t } = useTranslation();
  const pair = (key: string, values?: Record<string, string | number>) =>
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

  const pendingDevices = devices.filter(needsDeviceApproval);
  const trustedDevices = sortTrustedDevices(devices.filter((d) => d.trust === 'trusted'));
  const onlineCount = trustedDevices.filter((d) => deviceStatusKind(d) === 'connected').length;
  const offlineCount = trustedDevices.length - onlineCount;

  const hasPending = pendingDevices.length > 0;
  const hasTrusted = trustedDevices.length > 0;
  const badgeColor = hasPending
    ? settingsTileColors.orange
    : onlineCount > 0
      ? statusGreen
      : settingsTileColors.blue;
  const badgeIcon = hasPending
    ? 'exclamationmark.shield.fill'
    : onlineCount > 0
      ? 'checkmark'
      : 'laptopcomputer.and.iphone';

  const overviewTitle = hasPending
    ? pair('overviewPendingTitle')
    : onlineCount > 0
      ? pair('overviewReadyTitle')
      : hasTrusted
        ? pair('overviewOfflineTitle')
        : pair('overviewEmptyTitle');

  const overviewBody = hasPending
    ? pair('overviewPendingBody', { count: pendingDevices.length })
    : onlineCount > 0
      ? offlineCount > 0
        ? t('settingsSync:space.overview.deviceSummary', {
            online: onlineCount,
            offline: offlineCount,
          })
        : pair('overviewOnlineOnly', { count: onlineCount })
      : hasTrusted
        ? pair('overviewAllOffline', { count: trustedDevices.length })
        : pair('overviewEmptyBody');

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
        {/* ── Overview Hero Card ── */}
        <Section>
          <VStack spacing={14} alignment="leading" modifiers={[padding({ vertical: 6 })]}>
            <HStack
              spacing={14}
              alignment="top"
              modifiers={[frame({ maxWidth: Infinity, alignment: 'leading' })]}
            >
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
              <VStack alignment="leading" spacing={3}>
                <SwiftUIText modifiers={[font({ size: 17, weight: 'semibold' })]}>
                  {overviewTitle}
                </SwiftUIText>
                <SwiftUIText modifiers={[font({ size: 14 }), foregroundStyle('secondary')]}>
                  {overviewBody}
                </SwiftUIText>
              </VStack>
              <Spacer />
            </HStack>

            {hasPending ? (
              <HStack spacing={10} modifiers={[padding({ leading: 58 })]}>
                <SwiftUIButton
                  testID="devices-review-now"
                  onPress={() => onReviewPending(pendingDevices[0])}
                  modifiers={capsuleButton(true)}
                >
                  <HStack spacing={6}>
                    <Image systemName="checkmark.shield" size={14} />
                    <SwiftUIText modifiers={[font({ weight: 'semibold' })]}>
                      {pair('reviewNow')}
                    </SwiftUIText>
                  </HStack>
                </SwiftUIButton>
              </HStack>
            ) : null}

            <HStack spacing={10} modifiers={[padding({ leading: 58 })]}>
              <SwiftUIButton
                testID="devices-scan"
                onPress={onOpenScanJoin}
                modifiers={capsuleButton(!hasPending)}
              >
                <HStack spacing={6}>
                  <Image systemName="qrcode.viewfinder" size={14} />
                  <SwiftUIText modifiers={[font({ weight: 'semibold' })]}>
                    {pair('scanShort')}
                  </SwiftUIText>
                </HStack>
              </SwiftUIButton>
              <SwiftUIButton
                testID="devices-join"
                onPress={() => onOpenCodeSheet('enter')}
                modifiers={capsuleButton(false)}
              >
                <HStack spacing={6}>
                  <Image systemName="number" size={14} />
                  <SwiftUIText modifiers={[font({ weight: 'semibold' })]}>
                    {pair('codeShort')}
                  </SwiftUIText>
                </HStack>
              </SwiftUIButton>
            </HStack>

            {pairingCode ? (
              <SwiftUIButton
                testID="devices-active-code-banner"
                onPress={() => onOpenCodeSheet('show')}
                modifiers={[buttonStyle('plain')]}
              >
                <HStack
                  spacing={10}
                  alignment="center"
                  modifiers={[
                    frame({ maxWidth: Infinity }),
                    padding({ horizontal: 14, vertical: 10 }),
                    background(
                      PlatformColor('tertiarySystemFill'),
                      shapes.roundedRectangle({ cornerRadius: 12 }),
                    ),
                  ]}
                >
                  <VStack alignment="leading" spacing={2}>
                    <SwiftUIText modifiers={[font({ size: 12 }), foregroundStyle('secondary')]}>
                      {pairingCodeCountdown
                        ? pair('activeCodeBanner', { time: pairingCodeCountdown })
                        : pair('myCodeTab')}
                    </SwiftUIText>
                    <SwiftUIText
                      modifiers={[font({ size: 18, weight: 'bold', design: 'monospaced' })]}
                    >
                      {formatPairingCodeDisplay(pairingCode.code)}
                    </SwiftUIText>
                  </VStack>
                  <Spacer />
                  <Image systemName="chevron.right" size={13} color={chevronColor} />
                </HStack>
              </SwiftUIButton>
            ) : null}
          </VStack>
        </Section>

        {/* ── Pending Approval Devices (only shown if any remain pending) ── */}
        {pendingDevices.length > 0 ? (
          <Section
            header={<SwiftUIText>{`${pair('pending')} · ${pendingDevices.length}`}</SwiftUIText>}
          >
            {pendingDevices.map((device) => (
              <EngineDeviceRow
                key={device.id}
                testID={`pending-${device.id}`}
                device={device}
                subtitle={formatFingerprintShort(device.fingerprint)}
                statusLabel={pair('reviewNow')}
                dotColor={settingsTileColors.orange}
                onPress={() => onReviewPending(device)}
              />
            ))}
          </Section>
        ) : null}

        {/* ── Paired Devices ── */}
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

        {/* ── Suggested Devices (introduced by trusted peers) ── */}
        {candidates.length > 0 ? (
          <Section
            header={<SwiftUIText>{`${pair('recommended')} · ${candidates.length}`}</SwiftUIText>}
          >
            {candidates.map((candidate) => {
              const introducer = candidate.introducers?.[0]?.name;
              const subtitle = candidate.requested
                ? pair('phase.connecting')
                : introducer
                  ? pair('recommendedBy', { name: introducer })
                  : pair('recommendedHint');
              return (
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
                        {subtitle}
                      </SwiftUIText>
                    </VStack>
                    <Spacer />
                    <Image systemName="chevron.right" size={13} color={chevronColor} />
                  </HStack>
                </SwiftUIButton>
              );
            })}
          </Section>
        ) : null}

        {/* ── This Device (compact, unobtrusive at bottom) ── */}
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
      </IosSheetForm>
    </IosSheetPage>
  );
}
