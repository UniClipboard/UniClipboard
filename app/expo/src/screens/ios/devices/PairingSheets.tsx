import { useEffect, useState } from 'react';
import { Image as RNImage, StyleSheet, View } from 'react-native';
import {
  Button as SwiftUIButton,
  HStack,
  Image,
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
  disabled,
  font,
  foregroundStyle,
  frame,
  multilineTextAlignment,
  onSubmit,
  padding,
  shapes,
  submitLabel,
  textInputAutocapitalization,
} from '@expo/ui/swift-ui/modifiers';
import { useTranslation } from 'react-i18next';

import { IosBottomSheet, IosSheetForm, IosSheetPage, SheetCircleButton } from '@/components/ui';
import {
  iosProminentButtonModifiers,
  iosSecondaryButtonModifiers,
} from '@/components/ui/iosButtonStyles.ios';
import type { EngineCandidate, SensitiveTicket } from '@/engine/types';
import { PairingScanner } from '@/screens/PairingScanner';
import { settingsTileColors } from '@/screens/settings/ios/common';

export function PairingJoinSheet({
  mode,
  focused,
  foreground,
  starting,
  onSubmitInput,
  onClose,
}: {
  mode: 'manual' | 'scan' | null;
  focused: boolean;
  foreground: boolean;
  starting: boolean;
  onSubmitInput: (input: string) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const pair = (key: string) => t(`settingsSync:pairing.${key}`);
  const [input, setInput] = useState('');
  const inputState = useNativeState('');

  useEffect(() => {
    if (mode === null) {
      setInput('');
      inputState.set('');
    }
  }, [mode, inputState]);

  const canSubmit = Boolean(input.trim()) && !starting;

  const handleSubmit = () => {
    if (!canSubmit) return;
    onSubmitInput(input);
  };

  return (
    <IosBottomSheet
      visible={mode !== null}
      onDismiss={onClose}
      detents={mode === 'scan' ? ['large'] : ['medium', 'large']}
    >
      {mode === 'scan' ? (
        <IosSheetPage
          title={pair('scan')}
          rightSlots={[
            <SheetCircleButton
              key="close"
              systemName="xmark"
              accessibilityLabel={t('common:action.close')}
              onPress={onClose}
            />,
          ]}
        >
          <VStack
            spacing={16}
            modifiers={[
              frame({ maxWidth: Infinity, maxHeight: Infinity, alignment: 'top' }),
              padding({ horizontal: 20, top: 12, bottom: 20 }),
            ]}
          >
            <RNHostView matchContents>
              <View style={styles.scannerContainer}>
                {focused && foreground ? <PairingScanner onRead={onSubmitInput} /> : null}
              </View>
            </RNHostView>
          </VStack>
        </IosSheetPage>
      ) : mode === 'manual' ? (
        <IosSheetPage
          title={pair('enterCode')}
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
          </IosSheetForm>
          <VStack modifiers={[padding({ horizontal: 20, top: 8, bottom: 20 })]}>
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
          </VStack>
        </IosSheetPage>
      ) : null}
    </IosBottomSheet>
  );
}

export function PairingTicketSheet({
  ticket,
  focused,
  foreground,
  onCopy,
  onClose,
}: {
  ticket: SensitiveTicket | null;
  focused: boolean;
  foreground: boolean;
  onCopy: (ticket: SensitiveTicket) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const pair = (key: string) => t(`settingsSync:pairing.${key}`);
  const visible = ticket !== null && focused && foreground;

  return (
    <IosBottomSheet visible={visible} onDismiss={onClose} detents={['large']}>
      {visible && ticket ? (
        <IosSheetPage
          title={pair('ticketTitle')}
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
              <VStack
                alignment="center"
                spacing={16}
                modifiers={[frame({ maxWidth: Infinity }), padding({ vertical: 12 })]}
              >
                {ticket.qr_png ? (
                  <VStack
                    modifiers={[
                      padding({ all: 12 }),
                      background('#FFFFFF', shapes.roundedRectangle({ cornerRadius: 16 })),
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
                <SwiftUIText
                  modifiers={[
                    font({ size: 14 }),
                    foregroundStyle('secondary'),
                    multilineTextAlignment('center'),
                  ]}
                >
                  {pair('ticketHelp')}
                </SwiftUIText>
              </VStack>
            </Section>
          </IosSheetForm>
          <VStack modifiers={[padding({ horizontal: 20, top: 8, bottom: 20 })]}>
            <SwiftUIButton
              onPress={() => onCopy(ticket)}
              modifiers={[
                ...iosProminentButtonModifiers(undefined, { fullWidth: true }),
                controlSize('large'),
              ]}
            >
              <HStack modifiers={[frame({ maxWidth: Infinity })]}>
                <Spacer />
                <SwiftUIText modifiers={[font({ weight: 'semibold' })]}>
                  {t('common:action.copy')}
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
  const pair = (key: string) => t(`settingsSync:pairing.${key}`);
  const displayName = candidate ? candidate.name || candidate.device_id.slice(0, 8) : '';

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
                  <SwiftUIText
                    modifiers={[
                      font({ size: 12, design: 'monospaced' }),
                      foregroundStyle('secondary'),
                    ]}
                  >
                    {candidate.device_id.slice(0, 12)}
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
  scannerContainer: {
    width: '100%',
    minHeight: 280,
    gap: 12,
  },
  qrImage: {
    width: 240,
    height: 240,
  },
});
