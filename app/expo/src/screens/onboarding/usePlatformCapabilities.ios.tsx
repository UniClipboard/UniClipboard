import { useCallback, useEffect, useState } from 'react';
import { AppState, Linking, StyleSheet, View } from 'react-native';
import { useTranslation } from 'react-i18next';
import {
  Button as SwiftUIButton,
  HStack,
  Host,
  Image,
  Section,
  Spacer,
  Text as SwiftUIText,
  VStack,
  ZStack,
} from '@expo/ui/swift-ui';
import {
  controlSize,
  font,
  foregroundStyle,
  frame,
  padding,
  tint,
} from '@expo/ui/swift-ui/modifiers';
import { IosBottomSheet, IosSheetForm, IosSheetPage, SheetCircleButton } from '@/components/ui';
import { iosProminentButtonModifiers } from '@/components/ui/iosButtonStyles.ios';
import { unicEngineNative } from '@/engine/nativeModule';
import { useEngineStore } from '@/engine/store';
import type { KeyboardExtensionStatus } from '@/engine/types';
import { ClipboardSettingsGuideSheet } from '@/screens/settings/ios/ClipboardSettingsGuideSheet';
import { ShareFavoritesGuideSheet } from '@/screens/settings/ios/ShareFavoritesGuideSheet';
import { SettingsIconTile, settingsTileColors } from '@/screens/settings/ios/common';
import { showUnavailable } from '@/shell/notice';
import { iosAccentColor } from '@/theme/iosDesignTokens';
import type { PlatformCapabilitiesController } from './usePlatformCapabilities.types';

const INITIAL_KB: KeyboardExtensionStatus = {
  added: false,
  has_full_access_seen: false,
  sound_enabled: true,
  haptic_enabled: true,
};

export function usePlatformCapabilities(active: boolean): PlatformCapabilitiesController {
  const { t } = useTranslation();
  const mode = useEngineStore((s) => s.mode);
  const [pastePrompted, setPastePrompted] = useState(false);
  const [kbStatus, setKbStatus] = useState<KeyboardExtensionStatus>(INITIAL_KB);
  const [pasteGuideOpen, setPasteGuideOpen] = useState(false);
  const [kbGuideOpen, setKbGuideOpen] = useState(false);
  const [shareGuideOpen, setShareGuideOpen] = useState(false);

  const refreshKeyboard = useCallback(async () => {
    if (mode !== 'live') return;
    try {
      await unicEngineNative.syncKeyboardSnapshot().catch(() => {});
      const next = await unicEngineNative.getKeyboardExtensionStatus();
      setKbStatus(next);
    } catch {
      // Keep last known keyboard status.
    }
  }, [mode]);

  useEffect(() => {
    if (!active) return;
    void refreshKeyboard();
    const sub = AppState.addEventListener('change', (state) => {
      if (state === 'active') void refreshKeyboard();
    });
    return () => sub.remove();
  }, [active, refreshKeyboard]);

  const triggerPastePrompt = useCallback(async () => {
    if (mode !== 'live') {
      showUnavailable();
      return;
    }
    try {
      await unicEngineNative.readSystemClipboardAndCommit();
    } catch {
      // Empty clipboard or user dismissed prompt; still reveal settings guide next.
    } finally {
      setPastePrompted(true);
      void useEngineStore.getState().refreshSnapshot();
    }
  }, [mode]);

  const kbReady = kbStatus.added && kbStatus.has_full_access_seen;

  const rows: PlatformCapabilitiesController['rows'] = [
    {
      id: 'ios-paste',
      icon: 'paste',
      iosTileColor: '#007AFF',
      title: t('onboarding:guide.caps.iosPasteTitle'),
      subtitle: t('onboarding:guide.caps.iosPasteSub'),
      actionLabel: pastePrompted
        ? t('onboarding:guide.caps.iosPasteGuide')
        : t('onboarding:guide.caps.iosPasteEnable'),
      actionTone: pastePrompted ? 'soft' : 'primary',
      onPress: pastePrompted ? () => setPasteGuideOpen(true) : () => void triggerPastePrompt(),
      testID: 'onboarding-cap-ios-paste',
    },
    {
      id: 'ios-keyboard',
      icon: 'keyboard',
      iosTileColor: '#AF52DE',
      title: t('onboarding:guide.caps.iosKeyboardTitle'),
      subtitle: t('onboarding:guide.caps.iosKeyboardSub'),
      actionLabel: kbReady
        ? t('onboarding:guide.caps.iosKeyboardReady')
        : t('onboarding:guide.caps.iosKeyboardSetup'),
      actionTone: kbReady ? 'ok' : 'soft',
      onPress: () => setKbGuideOpen(true),
      testID: 'onboarding-cap-ios-keyboard',
    },
    {
      id: 'ios-share',
      icon: 'share',
      iosTileColor: '#34C759',
      title: t('onboarding:guide.caps.iosShareTitle'),
      subtitle: t('onboarding:guide.caps.iosShareSub'),
      actionLabel: t('onboarding:guide.caps.iosShareAvailable'),
      actionTone: 'ok',
      onPress: () => setShareGuideOpen(true),
      testID: 'onboarding-cap-ios-share',
    },
  ];

  const readyItems: PlatformCapabilitiesController['readyItems'] = [
    {
      id: 'ios-paste',
      icon: 'paste',
      iosTileColor: '#007AFF',
      title: t('onboarding:guide.ready.clipboardAccess'),
      statusLabel: pastePrompted
        ? t('onboarding:guide.caps.iosPasteDone')
        : t('onboarding:guide.ready.notEnabled'),
      isPositive: pastePrompted,
    },
    {
      id: 'ios-keyboard',
      icon: 'keyboard',
      iosTileColor: '#AF52DE',
      title: t('onboarding:guide.ready.unicKeyboard'),
      statusLabel: kbReady
        ? t('onboarding:guide.ready.ready')
        : t('onboarding:guide.ready.notEnabled'),
      isPositive: kbReady,
    },
  ];

  const renderOverlays = () => (
    <View style={s.hostWrap} pointerEvents="none">
      <Host style={s.host}>
        <ZStack modifiers={iosAccentColor ? [tint(iosAccentColor)] : []}>
          <ClipboardSettingsGuideSheet
            visible={pasteGuideOpen}
            onClose={() => setPasteGuideOpen(false)}
          />
          <ShareFavoritesGuideSheet
            visible={shareGuideOpen}
            onClose={() => setShareGuideOpen(false)}
          />
          <IosBottomSheet
            visible={kbGuideOpen}
            onDismiss={() => setKbGuideOpen(false)}
            detents={[{ fraction: 0.62 }, 'large']}
          >
            <IosSheetPage
              title={t('onboarding:guide.caps.iosKeyboardSheetTitle')}
              rightSlots={[
                <SheetCircleButton
                  key="close"
                  systemName="xmark"
                  accessibilityLabel={t('common:action.close')}
                  onPress={() => setKbGuideOpen(false)}
                />,
              ]}
            >
              <IosSheetForm>
                <Section>
                  <HStack spacing={12} alignment="center" modifiers={[padding({ vertical: 4 })]}>
                    <SettingsIconTile
                      systemName={kbStatus.added ? 'checkmark.circle.fill' : '1.circle.fill'}
                      color={kbStatus.added ? settingsTileColors.green : settingsTileColors.indigo}
                    />
                    <VStack alignment="leading" spacing={2}>
                      <SwiftUIText modifiers={[font({ weight: 'semibold' })]}>
                        {t('onboarding:guide.caps.iosKeyboardStep1Title')}
                      </SwiftUIText>
                      <SwiftUIText modifiers={[font({ size: 13 }), foregroundStyle('secondary')]}>
                        {t('onboarding:guide.caps.iosKeyboardStep1Path')}
                      </SwiftUIText>
                    </VStack>
                  </HStack>

                  <HStack spacing={12} alignment="center" modifiers={[padding({ vertical: 4 })]}>
                    <SettingsIconTile
                      systemName={kbReady ? 'checkmark.circle.fill' : '2.circle.fill'}
                      color={kbReady ? settingsTileColors.green : settingsTileColors.indigo}
                    />
                    <VStack alignment="leading" spacing={2}>
                      <SwiftUIText modifiers={[font({ weight: 'semibold' })]}>
                        {t('onboarding:guide.caps.iosKeyboardStep2Title')}
                      </SwiftUIText>
                      <SwiftUIText modifiers={[font({ size: 13 }), foregroundStyle('secondary')]}>
                        {t('onboarding:guide.caps.iosKeyboardStep2Desc')}
                      </SwiftUIText>
                    </VStack>
                  </HStack>

                  <HStack spacing={12} alignment="center" modifiers={[padding({ vertical: 4 })]}>
                    <SettingsIconTile
                      systemName={kbReady ? 'checkmark.circle.fill' : '3.circle.fill'}
                      color={kbReady ? settingsTileColors.green : settingsTileColors.indigo}
                    />
                    <VStack alignment="leading" spacing={2}>
                      <SwiftUIText modifiers={[font({ weight: 'semibold' })]}>
                        {t('onboarding:guide.caps.iosKeyboardStep3Title')}
                      </SwiftUIText>
                      <SwiftUIText modifiers={[font({ size: 13 }), foregroundStyle('secondary')]}>
                        {t('onboarding:guide.caps.iosKeyboardStep3Desc')}
                      </SwiftUIText>
                    </VStack>
                  </HStack>
                </Section>
              </IosSheetForm>
              <VStack spacing={8} modifiers={[padding({ horizontal: 20, top: 8, bottom: 20 })]}>
                <SwiftUIButton
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
                      {t('onboarding:guide.caps.iosKeyboardOpenSettings')}
                    </SwiftUIText>
                    <Spacer />
                  </HStack>
                </SwiftUIButton>
              </VStack>
            </IosSheetPage>
          </IosBottomSheet>
        </ZStack>
      </Host>
    </View>
  );

  return {
    stepLabel: t('onboarding:guide.caps.iosStepLabel'),
    title: t('onboarding:guide.caps.iosTitle'),
    subtitle: t('onboarding:guide.caps.iosSubtitle'),
    rows,
    readyItems,
    renderOverlays,
  };
}

const s = StyleSheet.create({
  hostWrap: {
    position: 'absolute',
    width: 0,
    height: 0,
  },
  host: { width: 0, height: 0 },
});
