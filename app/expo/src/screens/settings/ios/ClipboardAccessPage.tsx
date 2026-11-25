import React, { useEffect, useState } from 'react';
import { DynamicColorIOS, Linking } from 'react-native';
import { useTranslation } from 'react-i18next';
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
  accessibilityHidden,
  background,
  controlSize,
  font,
  foregroundStyle,
  frame,
  multilineTextAlignment,
  padding,
  shadow,
  shapes,
} from '@expo/ui/swift-ui/modifiers';

import { IosSheetForm, IosSheetPage } from '@/components/ui';
import {
  iosProminentButtonModifiers,
  iosSecondaryButtonModifiers,
} from '@/components/ui/iosButtonStyles.ios';
import { unicEngineNative } from '@/engine/nativeModule';
import { useEngineStore } from '@/engine/store';
import { iosColors } from '@/theme/iosDesignTokens';
import {
  SettingsIconTile,
  SettingsNavRow,
  settingsTileColors,
  statusGreen,
  statusOrange,
} from './common';

const PROMPT_PANEL_BACKGROUND = iosColors?.systemGroupedBackground ?? '#F2F2F7';
const PROMPT_ALERT_BACKGROUND = DynamicColorIOS({
  light: '#FFFFFF',
  dark: '#2C2C2E',
});
const PROMPT_BUTTON_BACKGROUND = iosColors?.tertiarySystemFill ?? '#E5E5EA';
const PROMPT_ALLOW_BACKGROUND = DynamicColorIOS({
  light: '#E8F1FC',
  dark: '#0B2A4A',
});
const TRIGGERED_BACKGROUND = DynamicColorIOS({
  light: '#E6F6EA',
  dark: '#12321C',
});

/**
 * 剪贴板访问权限页（对齐 v1 ClipboardAccessPage.tsx）：
 * Hero 卡片 + 模拟系统「允许粘贴」弹窗插图 + App 内一键触发授权按钮 + 实时 Engine 能力状态 + 图文引导/系统设置兜底。
 */
export function ClipboardAccessPage({ onOpenSettingsGuide }: { onOpenSettingsGuide: () => void }) {
  const { t } = useTranslation();
  const capabilities = useEngineStore((s) => s.capabilities);
  const [triggered, setTriggered] = useState(false);

  useEffect(() => {
    void useEngineStore.getState().refreshSnapshot();
  }, []);

  const triggerPastePermission = async () => {
    try {
      await unicEngineNative.readSystemClipboardAndCommit();
    } catch {
      // ignore empty clipboard or cancelled prompt
    } finally {
      setTriggered(true);
      void useEngineStore.getState().refreshSnapshot();
    }
  };

  return (
    <IosSheetPage title={t('settings:ios.extensions.clipboardAccess')}>
      <IosSheetForm>
        {/* ── 主操作：App 内触发一次，弹窗直接点「允许粘贴」 ── */}
        <Section>
          <VStack
            alignment="leading"
            spacing={16}
            modifiers={[
              frame({ maxWidth: Infinity, alignment: 'leading' }),
              padding({ vertical: 8 }),
            ]}
          >
            <HStack spacing={12}>
              <SettingsIconTile systemName="doc.on.clipboard" color={settingsTileColors.blue} />
              <SwiftUIText modifiers={[font({ size: 20, weight: 'bold' })]}>
                {t('settingsPermissions:clipboardAccess.hero.title')}
              </SwiftUIText>
            </HStack>
            <SwiftUIText modifiers={[font({ size: 15 }), foregroundStyle('secondary')]}>
              {t('settingsPermissions:clipboardAccess.hero.description')}
            </SwiftUIText>
            <PastePromptIllustration />
            {triggered ? (
              <HStack
                spacing={10}
                alignment="top"
                modifiers={[
                  frame({ maxWidth: Infinity, alignment: 'leading' }),
                  padding({ horizontal: 14, vertical: 12 }),
                  background(TRIGGERED_BACKGROUND, shapes.roundedRectangle({ cornerRadius: 14 })),
                ]}
              >
                <Image
                  systemName="checkmark.circle.fill"
                  size={18}
                  color={settingsTileColors.green}
                />
                <SwiftUIText
                  modifiers={[
                    font({ size: 15 }),
                    frame({ maxWidth: Infinity, alignment: 'leading' }),
                  ]}
                >
                  {t('settingsPermissions:clipboardAccess.hero.triggered')}
                </SwiftUIText>
              </HStack>
            ) : null}
            <SwiftUIButton
              testID="clipboard-access-trigger"
              onPress={() => void triggerPastePermission()}
              modifiers={[
                ...(triggered
                  ? iosSecondaryButtonModifiers({ fullWidth: true })
                  : iosProminentButtonModifiers(undefined, { fullWidth: true })),
                controlSize('large'),
              ]}
            >
              <HStack spacing={8} modifiers={[frame({ maxWidth: Infinity })]}>
                <Spacer />
                {triggered ? null : <Image systemName="hand.tap" size={16} />}
                <SwiftUIText modifiers={[font({ weight: 'semibold' })]}>
                  {triggered
                    ? t('settingsPermissions:clipboardAccess.hero.triggerAgain')
                    : t('settingsPermissions:clipboardAccess.hero.trigger')}
                </SwiftUIText>
                <Spacer />
              </HStack>
            </SwiftUIButton>
            {triggered ? null : (
              <SwiftUIText
                modifiers={[
                  font({ size: 13 }),
                  foregroundStyle('secondary'),
                  multilineTextAlignment('center'),
                  frame({ maxWidth: Infinity }),
                ]}
              >
                {t('settingsPermissions:clipboardAccess.hero.emptyClipboardHint')}
              </SwiftUIText>
            )}
          </VStack>
        </Section>

        {/* ── 实时 Engine 剪贴板能力状态 ── */}
        {capabilities ? (
          <Section>
            <SettingsNavRow
              testID="clipboard-observe"
              title={t('settings:live.observe')}
              value={t(`settings:live.status.${capabilities.effective.observe}`)}
              valueColor={
                capabilities.effective.observe === 'available' ? statusGreen : statusOrange
              }
              readOnly
            />
            <SettingsNavRow
              testID="clipboard-apply"
              title={t('settings:live.apply')}
              value={t(`settings:live.status.${capabilities.effective.apply}`)}
              valueColor={capabilities.effective.apply === 'available' ? statusGreen : statusOrange}
              readOnly
            />
            {(capabilities.restrictions ?? []).map((reason) => (
              <SettingsNavRow
                key={reason}
                testID={`restriction-${reason}`}
                title={t(`settings:live.status.${reason}`, { defaultValue: reason })}
                readOnly
              />
            ))}
          </Section>
        ) : null}

        {/* ── 兜底：图文步骤 Sheet + 直达系统设置 ── */}
        <Section
          header={<SwiftUIText>{t('settingsPermissions:clipboardAccess.fallbackSection.header')}</SwiftUIText>}
          footer={<SwiftUIText>{t('settingsPermissions:clipboardAccess.fallbackSection.footer')}</SwiftUIText>}
        >
          <SettingsNavRow
            testID="clipboard-access-settings-guide"
            icon="list.number"
            iconColor={settingsTileColors.gray}
            title={t('settingsPermissions:clipboardAccess.fallbackSection.guideRow')}
            subtitle={t('settingsPermissions:clipboardAccess.fallbackSection.guideRowSubtitle')}
            onPress={onOpenSettingsGuide}
          />
          <SettingsNavRow
            testID="clipboard-system-settings"
            icon="gearshape.fill"
            iconColor={settingsTileColors.gray}
            title={t('settingsPermissions:clipboardAccess.fallbackSection.openSettings')}
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

function PastePromptIllustration() {
  const { t } = useTranslation('settingsPermissions');
  return (
    <HStack
      modifiers={[
        frame({ maxWidth: Infinity }),
        padding({ all: 16 }),
        background(PROMPT_PANEL_BACKGROUND, shapes.roundedRectangle({ cornerRadius: 18 })),
        accessibilityHidden(true),
      ]}
    >
      <Spacer />
      <VStack
        spacing={14}
        modifiers={[
          frame({ width: 260 }),
          padding({ horizontal: 14, top: 18, bottom: 14 }),
          background(PROMPT_ALERT_BACKGROUND, shapes.roundedRectangle({ cornerRadius: 26 })),
          shadow({ radius: 12, y: 6, color: '#0000001F' }),
        ]}
      >
        <VStack spacing={4}>
          <SwiftUIText
            modifiers={[font({ size: 15, weight: 'semibold' }), multilineTextAlignment('center')]}
          >
            {t('clipboardAccess.prompt.title')}
          </SwiftUIText>
          <SwiftUIText
            modifiers={[
              font({ size: 13 }),
              foregroundStyle('secondary'),
              multilineTextAlignment('center'),
            ]}
          >
            {t('clipboardAccess.prompt.message')}
          </SwiftUIText>
        </VStack>
        <HStack spacing={8}>
          <SwiftUIText
            modifiers={[
              font({ size: 13, weight: 'semibold' }),
              foregroundStyle(settingsTileColors.blue),
              multilineTextAlignment('center'),
              frame({ maxWidth: Infinity, minHeight: 40 }),
              padding({ horizontal: 6 }),
              background(PROMPT_ALLOW_BACKGROUND, shapes.capsule()),
            ]}
          >
            {t('clipboardAccess.prompt.allow')}
          </SwiftUIText>
          <SwiftUIText
            modifiers={[
              font({ size: 13, weight: 'semibold' }),
              foregroundStyle('secondary'),
              multilineTextAlignment('center'),
              frame({ maxWidth: Infinity, minHeight: 40 }),
              padding({ horizontal: 6 }),
              background(PROMPT_BUTTON_BACKGROUND, shapes.capsule()),
            ]}
          >
            {t('clipboardAccess.prompt.deny')}
          </SwiftUIText>
        </HStack>
      </VStack>
      <Spacer />
    </HStack>
  );
}
