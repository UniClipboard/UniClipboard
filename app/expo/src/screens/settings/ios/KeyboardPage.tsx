import React, { useCallback, useEffect, useState } from 'react';
import { AppState, Linking } from 'react-native';
import { useTranslation } from 'react-i18next';
import {
  Button as SwiftUIButton,
  HStack,
  Image,
  Section,
  Spacer,
  Text as SwiftUIText,
  TextField,
  VStack,
} from '@expo/ui/swift-ui';
import {
  controlSize,
  font,
  foregroundStyle,
  frame,
  padding,
} from '@expo/ui/swift-ui/modifiers';

import { IosSheetForm, IosSheetPage } from '@/components/ui';
import { iosProminentButtonModifiers } from '@/components/ui/iosButtonStyles.ios';
import { unicEngineNative } from '@/engine/nativeModule';
import type { KeyboardExtensionStatus } from '@/engine/types';
import {
  IconToggleRow,
  SettingsIconTile,
  SettingsNavRow,
  settingsTileColors,
  statusGreen,
  statusOrange,
} from './common';

const INITIAL_STATUS: KeyboardExtensionStatus = {
  added: false,
  has_full_access_seen: false,
  sound_enabled: true,
  haptic_enabled: true,
};

/** iOS 键盘扩展引导与按键反馈设置页 */
export function KeyboardPage() {
  const { t } = useTranslation('settingsIos');
  const [status, setStatus] = useState<KeyboardExtensionStatus>(INITIAL_STATUS);
  const [testText, setTestText] = useState('');

  const refresh = useCallback(async () => {
    try {
      await unicEngineNative.syncKeyboardSnapshot().catch(() => {});
      const next = await unicEngineNative.getKeyboardExtensionStatus();
      setStatus(next);
    } catch {
      // Keep last known status when unavailable.
    }
  }, []);

  useEffect(() => {
    void refresh();
    const sub = AppState.addEventListener('change', (nextState) => {
      if (nextState === 'active') {
        void refresh();
      }
    });
    return () => sub.remove();
  }, [refresh]);

  const updateFeedback = useCallback(
    async (soundEnabled: boolean, hapticEnabled: boolean) => {
      setStatus((prev) => ({
        ...prev,
        sound_enabled: soundEnabled,
        haptic_enabled: hapticEnabled,
      }));
      try {
        const next = await unicEngineNative.setKeyboardPreferences(soundEnabled, hapticEnabled);
        setStatus(next);
      } catch {
        void refresh();
      }
    },
    [refresh],
  );

  const isReady = status.added && status.has_full_access_seen;
  const doneSteps = (status.added ? 1 : 0) + (status.has_full_access_seen ? 2 : 0);

  const heroTitle = isReady
    ? t('keyboard.ready.title')
    : status.added
      ? t('keyboard.almostThere.title')
      : t('keyboard.hero.title');
  const heroDesc = isReady
    ? t('keyboard.ready.description')
    : status.added
      ? t('keyboard.almostThere.description')
      : t('keyboard.hero.description');

  return (
    <IosSheetPage title={t('keyboard.title')}>
      <IosSheetForm>
        <Section>
          <VStack
            alignment="leading"
            spacing={14}
            modifiers={[
              frame({ maxWidth: Infinity, alignment: 'leading' }),
              padding({ vertical: 8 }),
            ]}
          >
            <HStack spacing={12}>
              <SettingsIconTile
                systemName={isReady ? 'checkmark.circle.fill' : 'keyboard'}
                color={isReady ? settingsTileColors.green : settingsTileColors.indigo}
              />
              <SwiftUIText modifiers={[font({ size: 20, weight: 'bold' })]}>
                {heroTitle}
              </SwiftUIText>
            </HStack>
            <SwiftUIText modifiers={[font({ size: 15 }), foregroundStyle('secondary')]}>
              {heroDesc}
            </SwiftUIText>
            {!isReady ? (
              <SwiftUIButton
                testID="keyboard-open-settings"
                onPress={() => {
                  void Linking.openSettings();
                }}
                modifiers={[
                  ...iosProminentButtonModifiers(undefined, { fullWidth: true }),
                  controlSize('large'),
                ]}
              >
                <HStack spacing={8} modifiers={[frame({ maxWidth: Infinity })]}>
                  <Spacer />
                  <Image systemName="gearshape.fill" size={15} />
                  <SwiftUIText modifiers={[font({ weight: 'semibold' })]}>
                    {t('keyboard.openSettings')}
                  </SwiftUIText>
                  <Spacer />
                </HStack>
              </SwiftUIButton>
            ) : null}
          </VStack>
        </Section>

        <Section
          header={<SwiftUIText>{t('keyboard.setup.title', { done: doneSteps })}</SwiftUIText>}
          footer={<SwiftUIText>{t('keyboard.openSettingsHint')}</SwiftUIText>}
        >
          <SettingsNavRow
            testID="keyboard-step-add"
            icon={status.added ? 'checkmark.circle.fill' : '1.circle.fill'}
            iconColor={status.added ? settingsTileColors.green : settingsTileColors.indigo}
            title={t('keyboard.setup.add.title')}
            subtitle={t('keyboard.setup.add.subtitle')}
            value={status.added ? t('keyboard.status.done') : t('keyboard.status.notAdded')}
            valueColor={status.added ? statusGreen : statusOrange}
            readOnly
          />
          <SettingsNavRow
            testID="keyboard-step-full-access"
            icon={status.has_full_access_seen ? 'checkmark.circle.fill' : '2.circle.fill'}
            iconColor={
              status.has_full_access_seen ? settingsTileColors.green : settingsTileColors.indigo
            }
            title={t('keyboard.setup.fullAccess.title')}
            subtitle={t('keyboard.setup.fullAccess.path')}
            value={
              status.has_full_access_seen ? t('keyboard.status.on') : t('keyboard.status.notChecked')
            }
            valueColor={status.has_full_access_seen ? statusGreen : undefined}
            readOnly
          />
          <SettingsNavRow
            testID="keyboard-step-switch"
            icon={status.has_full_access_seen ? 'checkmark.circle.fill' : '3.circle.fill'}
            iconColor={
              status.has_full_access_seen ? settingsTileColors.green : settingsTileColors.indigo
            }
            title={t('keyboard.setup.switch.title')}
            subtitle={t('keyboard.setup.switch.instructions')}
            value={
              status.has_full_access_seen
                ? t('keyboard.status.detected')
                : t('keyboard.status.notChecked')
            }
            valueColor={status.has_full_access_seen ? statusGreen : undefined}
            readOnly
          />
        </Section>

        <Section
          header={<SwiftUIText>{t('keyboard.howToUse.tryLabel')}</SwiftUIText>}
          footer={
            <SwiftUIText>
              {status.has_full_access_seen
                ? t('keyboard.setup.switch.detected')
                : t('keyboard.setup.switch.stillOffHint')}
            </SwiftUIText>
          }
        >
          <TextField
            testID="keyboard-try-input"
            placeholder={t('keyboard.howToUse.tryPlaceholder')}
            onTextChange={(next) => {
              setTestText(next);
              void refresh();
            }}
          />
          {testText.length > 0 ? (
            <SwiftUIText modifiers={[font({ size: 12 }), foregroundStyle('secondary')]}>
              {testText}
            </SwiftUIText>
          ) : null}
        </Section>

        <Section
          header={<SwiftUIText>{t('keyboard.feedback.title')}</SwiftUIText>}
          footer={<SwiftUIText>{t('keyboard.feedback.footer')}</SwiftUIText>}
        >
          <IconToggleRow
            testID="keyboard-feedback-sound"
            icon="speaker.wave.2.fill"
            iconColor={settingsTileColors.blue}
            label={t('keyboard.feedback.sound')}
            isOn={status.sound_enabled}
            onIsOnChange={(v) => void updateFeedback(v, status.haptic_enabled)}
          />
          <IconToggleRow
            testID="keyboard-feedback-haptic"
            icon="iphone.radiowaves.left.and.right"
            iconColor={settingsTileColors.purple}
            label={t('keyboard.feedback.haptic')}
            isOn={status.haptic_enabled}
            onIsOnChange={(v) => void updateFeedback(status.sound_enabled, v)}
          />
        </Section>

        <Section footer={<SwiftUIText>{t('keyboard.systemSettings.footer')}</SwiftUIText>}>
          <SettingsNavRow
            testID="keyboard-system-settings"
            icon="gearshape.fill"
            iconColor={settingsTileColors.gray}
            title={t('keyboard.systemSettings.row')}
            showsChevron={false}
            onPress={() => {
              void Linking.openSettings();
            }}
          />
        </Section>
      </IosSheetForm>
    </IosSheetPage>
  );
}
