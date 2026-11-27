import { useEffect, useState } from 'react';
import { Image as RNImage, PlatformColor, StyleSheet } from 'react-native';
import {
  Button as SwiftUIButton,
  HStack,
  Image,
  Picker,
  ProgressView,
  RNHostView,
  Section,
  Spacer,
  Text as SwiftUIText,
  TextField,
  useNativeState,
  VStack,
} from '@expo/ui/swift-ui';
import {
  autocorrectionDisabled,
  background,
  controlSize,
  cornerRadius,
  disabled,
  font,
  foregroundStyle,
  frame,
  lineLimit,
  multilineTextAlignment,
  onSubmit,
  padding,
  pickerStyle,
  shapes,
  submitLabel,
  tag,
  textInputAutocapitalization,
  truncationMode,
} from '@expo/ui/swift-ui/modifiers';
import { useTranslation } from 'react-i18next';

import { IosBottomSheet, IosSheetForm, IosSheetPage, SheetCircleButton } from '@/components/ui';
import {
  iosDestructiveButtonModifiers,
  iosProminentButtonModifiers,
  iosSecondaryButtonModifiers,
} from '@/components/ui/iosButtonStyles.ios';
import type { EngineCandidate, EngineDevice, PairingCode, SensitiveTicket } from '@/engine/types';
import type { PairingFlowStage, PairingFlowTab } from '@/screens/pairing/usePairingFlow';
import {
  deviceDisplayName,
  formatFingerprintLines,
  formatPairingCodeDisplay,
} from '@/screens/pairing/pairingUtils';
import { SettingsNavRow, settingsTileColors, statusGreen } from '@/screens/settings/ios/common';

export interface PairingFlowSheetProps {
  visible: boolean;
  tab: PairingFlowTab;
  stage: PairingFlowStage;
  busy: boolean;
  errorMessage: string | null;
  creatingCode: boolean;
  codeError: string | null;
  pairingCode: PairingCode | null;
  countdown: string;
  ticket: SensitiveTicket | null;
  exportingTicket: boolean;
  verifyDevice: EngineDevice | null;
  approvedPeerName: string;
  handshakeStageIndex: number;
  onSelectTab: (tab: PairingFlowTab) => void;
  onSubmitInput: (input: string) => void;
  onSwitchToScan: () => void;
  onRegenerateCode: () => void;
  onCancelCode: () => void;
  onCopyCode: () => void;
  onExportTicket: () => void;
  onCopyTicket: () => void;
  onApprove: () => void;
  onReject: () => void;
  onRetryEntry: () => void;
  onClose: () => void;
}

/**
 * Unified native SwiftUI pairing sheet:
 * - Idle: Segmented switcher ("Enter code" | "This device's code" + optional offline QR/ticket)
 * - Connecting: 3-step live handshake progress
 * - Verify: Automatic inline fingerprint verification (Approve / Reject)
 * - Waiting remote / Connected: Clear completion feedback
 */
export function PairingFlowSheet({
  visible,
  tab,
  stage,
  busy,
  errorMessage,
  creatingCode,
  codeError,
  pairingCode,
  countdown,
  ticket,
  exportingTicket,
  verifyDevice,
  approvedPeerName,
  handshakeStageIndex,
  onSelectTab,
  onSubmitInput,
  onSwitchToScan,
  onRegenerateCode,
  onCancelCode,
  onCopyCode,
  onExportTicket,
  onCopyTicket,
  onApprove,
  onReject,
  onRetryEntry,
  onClose,
}: PairingFlowSheetProps) {
  const { t } = useTranslation();
  const pair = (key: string, values?: Record<string, string>) =>
    t(`settingsSync:pairing.${key}`, values);
  const [input, setInput] = useState('');
  const inputState = useNativeState('');

  useEffect(() => {
    if (!visible || stage !== 'idle') {
      setInput('');
      inputState.set('');
    }
  }, [visible, stage, inputState]);

  const canSubmit = Boolean(input.trim()) && !busy;
  const handleSubmit = () => {
    if (!canSubmit) return;
    onSubmitInput(input);
  };

  const title =
    stage === 'verify'
      ? pair('approveSheetTitle')
      : stage === 'connecting'
        ? pair('connectingTitle')
        : stage === 'waiting-remote'
          ? pair('waitingRemoteTitle')
          : stage === 'connected'
            ? pair('connectedSheetTitle')
            : stage === 'error'
              ? pair('phase.failed')
              : pair('codeSheetTitle');

  const canReject = Boolean(verifyDevice && verifyDevice.trust === 'pending');

  const handshakeSteps = [
    pair('phase.resolving'),
    pair('phase.connecting'),
    pair('phase.handshaking'),
  ];

  return (
    <IosBottomSheet
      visible={visible}
      onDismiss={onClose}
      detents={['large']}
      interactiveDismissDisabled={busy}
    >
      {visible ? (
        <IosSheetPage
          title={title}
          rightSlots={[
            <SheetCircleButton
              key="close"
              testID="devices-pairing-close"
              systemName="xmark"
              accessibilityLabel={t('common:action.close')}
              disabled={busy}
              onPress={onClose}
            />,
          ]}
        >
          {stage === 'idle' ? (
            <>
              <IosSheetForm>
                <Section>
                  <Picker
                    testID="devices-pairing-mode"
                    selection={tab}
                    onSelectionChange={(value) => onSelectTab(value as PairingFlowTab)}
                    modifiers={[pickerStyle('segmented')]}
                    label={<SwiftUIText>{pair('codeSheetTitle')}</SwiftUIText>}
                  >
                    <SwiftUIText modifiers={[tag('enter')]}>{pair('enterCodeTab')}</SwiftUIText>
                    <SwiftUIText modifiers={[tag('show')]}>{pair('myCodeTab')}</SwiftUIText>
                  </Picker>
                </Section>

                {tab === 'enter' ? (
                  <>
                    <Section footer={<SwiftUIText>{pair('enterCodeHelp')}</SwiftUIText>}>
                      <TextField
                        testID="devices-join-input"
                        text={inputState}
                        placeholder={pair('inputHint')}
                        autoFocus
                        onTextChange={setInput}
                        modifiers={[
                          autocorrectionDisabled(true),
                          textInputAutocapitalization('never'),
                          submitLabel('join'),
                          onSubmit(handleSubmit),
                        ]}
                      />
                    </Section>
                    {errorMessage ? (
                      <Section>
                        <SwiftUIText
                          modifiers={[font({ size: 14 }), foregroundStyle(settingsTileColors.red)]}
                        >
                          {errorMessage}
                        </SwiftUIText>
                      </Section>
                    ) : null}
                  </>
                ) : (
                  <>
                    <Section footer={<SwiftUIText>{pair('myCodeAutoPopupHint')}</SwiftUIText>}>
                      <VStack
                        alignment="center"
                        spacing={12}
                        modifiers={[frame({ maxWidth: Infinity }), padding({ vertical: 14 })]}
                      >
                        {creatingCode && !pairingCode ? (
                          <ProgressView />
                        ) : pairingCode ? (
                          <>
                            <SwiftUIText
                              testID="devices-my-code-value"
                              modifiers={[
                                font({ size: 34, weight: 'bold', design: 'monospaced' }),
                                multilineTextAlignment('center'),
                              ]}
                            >
                              {formatPairingCodeDisplay(pairingCode.code)}
                            </SwiftUIText>
                            {countdown ? (
                              <SwiftUIText
                                modifiers={[font({ size: 13 }), foregroundStyle('secondary')]}
                              >
                                {pair('codeExpiresIn', { time: countdown })}
                              </SwiftUIText>
                            ) : null}
                          </>
                        ) : (
                          <SwiftUIText
                            modifiers={[
                              font({ size: 14 }),
                              foregroundStyle('secondary'),
                              multilineTextAlignment('center'),
                            ]}
                          >
                            {codeError || pair('codeInstructions')}
                          </SwiftUIText>
                        )}
                      </VStack>
                      {pairingCode ? (
                        <>
                          <SettingsNavRow
                            testID="devices-copy-code"
                            icon="doc.on.doc"
                            iconColor={settingsTileColors.blue}
                            title={pair('copyCode')}
                            showsChevron={false}
                            onPress={onCopyCode}
                          />
                          <SettingsNavRow
                            testID="devices-create-code"
                            icon="arrow.clockwise"
                            iconColor={settingsTileColors.indigo}
                            title={pair('regenerateCode')}
                            showsChevron={false}
                            onPress={onRegenerateCode}
                          />
                          <SettingsNavRow
                            testID="devices-cancel-code"
                            icon="xmark.circle"
                            iconColor={settingsTileColors.red}
                            title={pair('cancelCode')}
                            destructive
                            showsChevron={false}
                            onPress={onCancelCode}
                          />
                        </>
                      ) : (
                        <SettingsNavRow
                          testID="devices-create-code"
                          icon="number.circle.fill"
                          iconColor={settingsTileColors.blue}
                          title={pair('generateCode')}
                          showsChevron={false}
                          onPress={onRegenerateCode}
                        />
                      )}
                    </Section>

                    <Section
                      header={<SwiftUIText>{pair('offlineSectionTitle')}</SwiftUIText>}
                      footer={<SwiftUIText>{pair('offlineSectionHelp')}</SwiftUIText>}
                    >
                      {ticket ? (
                        <>
                          <VStack
                            alignment="center"
                            spacing={12}
                            modifiers={[frame({ maxWidth: Infinity }), padding({ vertical: 10 })]}
                          >
                            {ticket.qr_png ? (
                              <VStack
                                modifiers={[
                                  padding({ all: 12 }),
                                  background(
                                    '#FFFFFF',
                                    shapes.roundedRectangle({ cornerRadius: 16 }),
                                  ),
                                ]}
                              >
                                <RNHostView matchContents>
                                  <RNImage
                                    testID="devices-pairing-qr"
                                    accessibilityLabel={pair('ticketTitle')}
                                    source={{ uri: `data:image/png;base64,${ticket.qr_png}` }}
                                    style={styles.qrImage}
                                  />
                                </RNHostView>
                              </VStack>
                            ) : null}
                          </VStack>
                          <SettingsNavRow
                            testID="devices-copy-ticket"
                            icon="doc.on.doc"
                            iconColor={settingsTileColors.purple}
                            title={pair('copyTicket')}
                            showsChevron={false}
                            onPress={onCopyTicket}
                          />
                        </>
                      ) : (
                        <SettingsNavRow
                          testID="devices-export-ticket"
                          icon="qrcode"
                          iconColor={settingsTileColors.purple}
                          title={exportingTicket ? pair('preparingQR') : pair('showOfflineQR')}
                          disabled={exportingTicket}
                          showsChevron={false}
                          onPress={onExportTicket}
                        />
                      )}
                    </Section>
                  </>
                )}
              </IosSheetForm>

              {tab === 'enter' ? (
                <VStack spacing={10} modifiers={[padding({ horizontal: 20, top: 8, bottom: 20 })]}>
                  <SwiftUIButton
                    testID="devices-join-submit"
                    onPress={handleSubmit}
                    modifiers={[
                      ...iosProminentButtonModifiers(undefined, { fullWidth: true }),
                      controlSize('large'),
                      disabled(!canSubmit),
                    ]}
                  >
                    <HStack modifiers={[frame({ maxWidth: Infinity })]}>
                      <Spacer />
                      <SwiftUIText modifiers={[font({ weight: 'semibold' })]}>
                        {pair('connect')}
                      </SwiftUIText>
                      <Spacer />
                    </HStack>
                  </SwiftUIButton>
                  <SwiftUIButton
                    testID="devices-switch-scan"
                    onPress={onSwitchToScan}
                    modifiers={[
                      ...iosSecondaryButtonModifiers({ fullWidth: true }),
                      controlSize('large'),
                    ]}
                  >
                    <HStack spacing={6} modifiers={[frame({ maxWidth: Infinity })]}>
                      <Spacer />
                      <Image systemName="qrcode.viewfinder" size={16} />
                      <SwiftUIText modifiers={[font({ weight: 'semibold' })]}>
                        {pair('scanShort')}
                      </SwiftUIText>
                      <Spacer />
                    </HStack>
                  </SwiftUIButton>
                </VStack>
              ) : null}
            </>
          ) : stage === 'connecting' ? (
            <>
              <IosSheetForm>
                <Section>
                  <VStack
                    alignment="center"
                    spacing={16}
                    modifiers={[frame({ maxWidth: Infinity }), padding({ vertical: 18 })]}
                  >
                    <ProgressView />
                    <SwiftUIText
                      testID="devices-pairing-status"
                      modifiers={[font({ size: 17, weight: 'semibold' })]}
                    >
                      {handshakeSteps[handshakeStageIndex] ?? pair('phase.resolving')}
                    </SwiftUIText>
                    <VStack alignment="leading" spacing={10} modifiers={[padding({ top: 4 })]}>
                      {handshakeSteps.map((label, idx) => {
                        const done = idx < handshakeStageIndex;
                        const active = idx === handshakeStageIndex;
                        return (
                          <HStack key={label} spacing={10} alignment="center">
                            {done ? (
                              <Image systemName="checkmark.circle.fill" size={16} color={statusGreen} />
                            ) : active ? (
                              <ProgressView modifiers={[frame({ width: 16, height: 16 })]} />
                            ) : (
                              <Image
                                systemName="circle"
                                size={14}
                                color={PlatformColor('tertiaryLabel')}
                              />
                            )}
                            <SwiftUIText
                              modifiers={[
                                font({ size: 14, weight: active ? 'semibold' : 'regular' }),
                                foregroundStyle(done ? statusGreen : active ? 'primary' : 'secondary'),
                              ]}
                            >
                              {label}
                            </SwiftUIText>
                          </HStack>
                        );
                      })}
                    </VStack>
                  </VStack>
                </Section>
              </IosSheetForm>
              <VStack modifiers={[padding({ horizontal: 20, top: 8, bottom: 20 })]}>
                <SwiftUIButton
                  testID="devices-stop-waiting"
                  onPress={onClose}
                  modifiers={[
                    ...iosSecondaryButtonModifiers({ fullWidth: true }),
                    controlSize('large'),
                  ]}
                >
                  <HStack modifiers={[frame({ maxWidth: Infinity })]}>
                    <Spacer />
                    <SwiftUIText modifiers={[font({ weight: 'semibold' })]}>
                      {t('common:action.cancel')}
                    </SwiftUIText>
                    <Spacer />
                  </HStack>
                </SwiftUIButton>
              </VStack>
            </>
          ) : stage === 'verify' && verifyDevice ? (
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
                        {deviceDisplayName(verifyDevice)}
                      </SwiftUIText>
                      <SwiftUIText modifiers={[font({ size: 13 }), foregroundStyle('secondary')]}>
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
                        {formatFingerprintLines(verifyDevice.fingerprint)}
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
                      {pair('deviceID', { id: verifyDevice.id })}
                    </SwiftUIText>
                  </VStack>
                </Section>

                {errorMessage ? (
                  <Section>
                    <SwiftUIText
                      modifiers={[font({ size: 14 }), foregroundStyle(settingsTileColors.red)]}
                    >
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
                ) : null}
              </VStack>
            </>
          ) : stage === 'waiting-remote' ? (
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
                      {pair('waitingRemoteHeadline', { name: approvedPeerName })}
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
                  onPress={onClose}
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
          ) : stage === 'connected' ? (
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
                      {pair('connectedHeadline', { name: approvedPeerName })}
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
                  onPress={onClose}
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
                      systemName="exclamationmark.triangle.fill"
                      size={28}
                      color="white"
                      modifiers={[
                        frame({ width: 56, height: 56 }),
                        background(settingsTileColors.red),
                        cornerRadius(28),
                      ]}
                    />
                    <SwiftUIText
                      modifiers={[
                        font({ size: 18, weight: 'semibold' }),
                        multilineTextAlignment('center'),
                      ]}
                    >
                      {pair('phase.failed')}
                    </SwiftUIText>
                    <SwiftUIText
                      modifiers={[
                        font({ size: 14 }),
                        foregroundStyle('secondary'),
                        multilineTextAlignment('center'),
                      ]}
                    >
                      {errorMessage || pair('actionFailed')}
                    </SwiftUIText>
                  </VStack>
                </Section>
              </IosSheetForm>
              <VStack spacing={10} modifiers={[padding({ horizontal: 20, top: 8, bottom: 20 })]}>
                <SwiftUIButton
                  onPress={onRetryEntry}
                  modifiers={[
                    ...iosProminentButtonModifiers(undefined, { fullWidth: true }),
                    controlSize('large'),
                  ]}
                >
                  <HStack modifiers={[frame({ maxWidth: Infinity })]}>
                    <Spacer />
                    <SwiftUIText modifiers={[font({ weight: 'semibold' })]}>
                      {pair('tryAgain')}
                    </SwiftUIText>
                    <Spacer />
                  </HStack>
                </SwiftUIButton>
                <SwiftUIButton
                  onPress={onClose}
                  modifiers={[
                    ...iosSecondaryButtonModifiers({ fullWidth: true }),
                    controlSize('large'),
                  ]}
                >
                  <HStack modifiers={[frame({ maxWidth: Infinity })]}>
                    <Spacer />
                    <SwiftUIText modifiers={[font({ weight: 'semibold' })]}>
                      {t('common:action.close')}
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

export function CandidateDetailSheet({
  candidate,
  starting,
  onConnect,
  onIgnore,
  onClose,
}: {
  candidate: EngineCandidate | null;
  starting: boolean;
  onConnect: (candidate: EngineCandidate) => void;
  onIgnore: (candidate: EngineCandidate) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const pair = (key: string, values?: Record<string, string>) =>
    t(`settingsSync:pairing.${key}`, values);
  const displayName = candidate ? candidate.name || candidate.device_id.slice(0, 8) : '';
  const introducer = candidate?.introducers?.[0]?.name;

  return (
    <IosBottomSheet visible={candidate !== null} onDismiss={onClose} detents={['medium']}>
      {candidate ? (
        <IosSheetPage
          title={displayName}
          rightSlots={[
            <SheetCircleButton
              key="close"
              systemName="xmark"
              accessibilityLabel={t('common:action.close')}
              onPress={onClose}
            />,
          ]}
        >
          <IosSheetForm>
            <Section>
              <HStack spacing={12} alignment="center" modifiers={[frame({ maxWidth: Infinity })]}>
                <Image
                  systemName="laptopcomputer"
                  size={30}
                  color={settingsTileColors.blue}
                  modifiers={[frame({ width: 42, height: 42 })]}
                />
                <VStack alignment="leading" spacing={4}>
                  <SwiftUIText modifiers={[font({ weight: 'semibold' })]}>
                    {displayName}
                  </SwiftUIText>
                  <SwiftUIText modifiers={[font({ size: 13 }), foregroundStyle('secondary')]}>
                    {introducer ? pair('recommendedBy', { name: introducer }) : pair('recommendedHint')}
                  </SwiftUIText>
                </VStack>
              </HStack>
            </Section>
          </IosSheetForm>
          <VStack spacing={10} modifiers={[padding({ horizontal: 20, top: 8, bottom: 20 })]}>
            <SwiftUIButton
              onPress={() => onConnect(candidate)}
              modifiers={[
                ...iosProminentButtonModifiers(undefined, { fullWidth: true }),
                controlSize('large'),
                disabled(starting || candidate.requested),
              ]}
            >
              <HStack modifiers={[frame({ maxWidth: Infinity })]}>
                <Spacer />
                <SwiftUIText modifiers={[font({ weight: 'semibold' })]}>
                  {pair('connect')}
                </SwiftUIText>
                <Spacer />
              </HStack>
            </SwiftUIButton>
            <SwiftUIButton
              onPress={() => onIgnore(candidate)}
              modifiers={[
                ...iosSecondaryButtonModifiers({ fullWidth: true }),
                controlSize('large'),
              ]}
            >
              <HStack modifiers={[frame({ maxWidth: Infinity })]}>
                <Spacer />
                <SwiftUIText modifiers={[font({ weight: 'semibold' })]}>
                  {pair('ignore')}
                </SwiftUIText>
                <Spacer />
              </HStack>
            </SwiftUIButton>
          </VStack>
        </IosSheetPage>
      ) : null}
    </IosBottomSheet>
  );
}

const styles = StyleSheet.create({
  qrImage: {
    width: 220,
    height: 220,
  },
});
