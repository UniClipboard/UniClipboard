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
  background,
  controlSize,
  cornerRadius,
  disabled,
  font,
  foregroundStyle,
  frame,
  lineLimit,
  multilineTextAlignment,
  padding,
  shapes,
  truncationMode,
} from '@expo/ui/swift-ui/modifiers';
import { PlatformColor } from 'react-native';
import { useTranslation } from 'react-i18next';

import { IosBottomSheet, IosSheetForm, IosSheetPage, SheetCircleButton } from '@/components/ui';
import {
  iosDestructiveButtonModifiers,
  iosProminentButtonModifiers,
  iosSecondaryButtonModifiers,
} from '@/components/ui/iosButtonStyles.ios';
import { settingsTileColors, statusGreen } from '@/screens/settings/ios/common';
import type { PairingApprovalSheetProps } from './PairingApprovalSheet.types';
import { deviceDisplayName, formatFingerprintLines } from './pairingUtils';

/**
 * Global native iOS sheet that pops up automatically whenever a device needs
 * fingerprint confirmation, and guides through remote confirmation to connected.
 */
export function PairingApprovalSheet({
  visible,
  phase,
  device,
  peerName,
  busy,
  errorMessage,
  onApprove,
  onReject,
  onDismiss,
}: PairingApprovalSheetProps) {
  const { t } = useTranslation();
  const pair = (key: string, values?: Record<string, string>) =>
    t(`settingsSync:pairing.${key}`, values);

  const canReject = Boolean(device && device.trust === 'pending');
  const displayName = device ? deviceDisplayName(device) : peerName;

  const sheetTitle =
    phase === 'connected'
      ? pair('connectedSheetTitle')
      : phase === 'waiting-remote'
        ? pair('waitingRemoteTitle')
        : pair('approveSheetTitle');

  return (
    <IosBottomSheet
      standalone
      visible={visible}
      onDismiss={onDismiss}
      detents={[{ fraction: 0.68 }, 'large']}
      interactiveDismissDisabled={busy}
    >
      {visible ? (
        <IosSheetPage
          title={sheetTitle}
          rightSlots={[
            <SheetCircleButton
              key="close"
              testID="devices-approve-close"
              systemName="xmark"
              accessibilityLabel={t('common:action.close')}
              disabled={busy}
              onPress={onDismiss}
            />,
          ]}
        >
          {phase === 'verify' && device ? (
            <>
              <IosSheetForm>
                <Section>
                  <HStack spacing={14} alignment="center" modifiers={[frame({ maxWidth: Infinity })]}>
                    <Image
                      systemName="laptopcomputer"
                      size={22}
                      color="white"
                      modifiers={[
                        frame({ width: 44, height: 44 }),
                        background(settingsTileColors.orange),
                        cornerRadius(12),
                      ]}
                    />
                    <VStack alignment="leading" spacing={3}>
                      <SwiftUIText modifiers={[font({ size: 17, weight: 'semibold' })]}>
                        {displayName}
                      </SwiftUIText>
                      <SwiftUIText
                        modifiers={[
                          font({ size: 13 }),
                          foregroundStyle('secondary'),
                          lineLimit(2),
                        ]}
                      >
                        {pair('incomingSubtitle')}
                      </SwiftUIText>
                    </VStack>
                    <Spacer />
                  </HStack>
                </Section>

                <Section footer={<SwiftUIText>{pair('approveHint')}</SwiftUIText>}>
                  <VStack
                    alignment="center"
                    spacing={10}
                    modifiers={[frame({ maxWidth: Infinity }), padding({ vertical: 8 })]}
                  >
                    <SwiftUIText modifiers={[font({ size: 13 }), foregroundStyle('secondary')]}>
                      {pair('fingerprintLabel')}
                    </SwiftUIText>
                    <VStack
                      alignment="center"
                      modifiers={[
                        frame({ maxWidth: Infinity }),
                        padding({ horizontal: 14, vertical: 14 }),
                        background(
                          PlatformColor('tertiarySystemFill'),
                          shapes.roundedRectangle({ cornerRadius: 12 }),
                        ),
                      ]}
                    >
                      <SwiftUIText
                        testID="devices-approve-fingerprint"
                        modifiers={[
                          font({ size: 18, weight: 'semibold', design: 'monospaced' }),
                          multilineTextAlignment('center'),
                        ]}
                      >
                        {formatFingerprintLines(device.fingerprint)}
                      </SwiftUIText>
                    </VStack>
                    <SwiftUIText
                      modifiers={[
                        font({ size: 12, design: 'monospaced' }),
                        foregroundStyle('secondary'),
                        lineLimit(1),
                        truncationMode('middle'),
                      ]}
                    >
                      {pair('deviceID', { id: device.id })}
                    </SwiftUIText>
                  </VStack>
                </Section>

                {errorMessage ? (
                  <Section>
                    <SwiftUIText modifiers={[font({ size: 14 }), foregroundStyle(settingsTileColors.red)]}>
                      {errorMessage}
                    </SwiftUIText>
                  </Section>
                ) : null}
              </IosSheetForm>

              <VStack spacing={10} modifiers={[padding({ horizontal: 20, top: 8, bottom: 20 })]}>
                <SwiftUIButton
                  testID="devices-approve-confirm"
                  onPress={onApprove}
                  modifiers={[
                    ...iosProminentButtonModifiers(undefined, { fullWidth: true }),
                    controlSize('large'),
                    disabled(busy),
                  ]}
                >
                  <HStack spacing={8} modifiers={[frame({ maxWidth: Infinity })]}>
                    <Spacer />
                    {busy ? <ProgressView /> : null}
                    <SwiftUIText modifiers={[font({ weight: 'semibold' })]}>
                      {pair('approve')}
                    </SwiftUIText>
                    <Spacer />
                  </HStack>
                </SwiftUIButton>

                {canReject ? (
                  <SwiftUIButton
                    testID="devices-approve-reject"
                    onPress={onReject}
                    modifiers={[
                      ...iosDestructiveButtonModifiers({ fullWidth: true }),
                      controlSize('large'),
                      disabled(busy),
                    ]}
                  >
                    <HStack modifiers={[frame({ maxWidth: Infinity })]}>
                      <Spacer />
                      <SwiftUIText modifiers={[font({ weight: 'semibold' })]}>
                        {pair('reject')}
                      </SwiftUIText>
                      <Spacer />
                    </HStack>
                  </SwiftUIButton>
                ) : (
                  <SwiftUIButton
                    testID="devices-approve-cancel"
                    onPress={onDismiss}
                    modifiers={[
                      ...iosSecondaryButtonModifiers({ fullWidth: true }),
                      controlSize('large'),
                      disabled(busy),
                    ]}
                  >
                    <HStack modifiers={[frame({ maxWidth: Infinity })]}>
                      <Spacer />
                      <SwiftUIText modifiers={[font({ weight: 'semibold' })]}>
                        {pair('later')}
                      </SwiftUIText>
                      <Spacer />
                    </HStack>
                  </SwiftUIButton>
                )}
              </VStack>
            </>
          ) : phase === 'waiting-remote' ? (
            <>
              <IosSheetForm>
                <Section>
                  <VStack
                    alignment="center"
                    spacing={14}
                    modifiers={[frame({ maxWidth: Infinity }), padding({ vertical: 20 })]}
                  >
                    <ProgressView />
                    <SwiftUIText
                      modifiers={[
                        font({ size: 18, weight: 'semibold' }),
                        multilineTextAlignment('center'),
                      ]}
                    >
                      {pair('waitingRemoteHeadline', { name: displayName })}
                    </SwiftUIText>
                    <SwiftUIText
                      modifiers={[
                        font({ size: 14 }),
                        foregroundStyle('secondary'),
                        multilineTextAlignment('center'),
                      ]}
                    >
                      {pair('waitingRemoteBody')}
                    </SwiftUIText>
                  </VStack>
                </Section>
              </IosSheetForm>
              <VStack modifiers={[padding({ horizontal: 20, top: 8, bottom: 20 })]}>
                <SwiftUIButton
                  testID="devices-approve-done"
                  onPress={onDismiss}
                  modifiers={[
                    ...iosSecondaryButtonModifiers({ fullWidth: true }),
                    controlSize('large'),
                  ]}
                >
                  <HStack modifiers={[frame({ maxWidth: Infinity })]}>
                    <Spacer />
                    <SwiftUIText modifiers={[font({ weight: 'semibold' })]}>
                      {pair('waitInBackground')}
                    </SwiftUIText>
                    <Spacer />
                  </HStack>
                </SwiftUIButton>
              </VStack>
            </>
          ) : (
            <>
              <IosSheetForm>
                <Section>
                  <VStack
                    alignment="center"
                    spacing={14}
                    modifiers={[frame({ maxWidth: Infinity }), padding({ vertical: 20 })]}
                  >
                    <Image
                      systemName="checkmark"
                      size={28}
                      color="white"
                      modifiers={[
                        frame({ width: 56, height: 56 }),
                        background(statusGreen),
                        cornerRadius(28),
                      ]}
                    />
                    <SwiftUIText
                      modifiers={[
                        font({ size: 19, weight: 'semibold' }),
                        multilineTextAlignment('center'),
                      ]}
                    >
                      {pair('connectedHeadline', { name: displayName })}
                    </SwiftUIText>
                    <SwiftUIText
                      modifiers={[
                        font({ size: 14 }),
                        foregroundStyle('secondary'),
                        multilineTextAlignment('center'),
                      ]}
                    >
                      {pair('connectedBody')}
                    </SwiftUIText>
                  </VStack>
                </Section>
              </IosSheetForm>
              <VStack modifiers={[padding({ horizontal: 20, top: 8, bottom: 20 })]}>
                <SwiftUIButton
                  testID="devices-approve-done"
                  onPress={onDismiss}
                  modifiers={[
                    ...iosProminentButtonModifiers(undefined, { fullWidth: true }),
                    controlSize('large'),
                  ]}
                >
                  <HStack modifiers={[frame({ maxWidth: Infinity })]}>
                    <Spacer />
                    <SwiftUIText modifiers={[font({ weight: 'semibold' })]}>
                      {t('common:action.done')}
                    </SwiftUIText>
                    <Spacer />
                  </HStack>
                </SwiftUIButton>
              </VStack>
            </>
          )}
        </IosSheetPage>
      ) : null}
    </IosBottomSheet>
  );
}
