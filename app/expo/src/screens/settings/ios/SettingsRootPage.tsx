import { useEffect } from 'react';
import { Alert } from 'react-native';
import { Section, Text as SwiftUIText } from '@expo/ui/swift-ui';
import { nativeApplicationVersion } from 'expo-application';
import { useTranslation } from 'react-i18next';
import { useShallow } from 'zustand/react/shallow';

import { IosSheetForm, IosSheetPage } from '@/components/ui';
import { useEngineStore } from '@/engine/store';
import { useTheme } from '@/hooks/useTheme';
import { useAppLanguage } from '@/i18n/useAppLanguage';
import { cloudStatusLabel } from '@/lib/cloudAuth';
import { useCloudStore } from '@/stores/cloudStore';
import {
  IconToggleRow,
  SettingsNavRow,
  SettingsPickerRow,
  settingsTileColors,
  statusOrange,
} from './common';
import type { IosSettingsPage } from './types';

/**
 * iOS「设置」标签页根页（对齐 v1 SettingsRootPage.tsx）：
 * 剪贴板同步方向 → 通用（历史 / 剪贴板访问 / 主题 / 语言 / 存储 / 高级网络）
 * → 扩展与权限（分享）→ 支持（诊断日志）→ 其他（隐私 / 关于）。
 */
export function SettingsRootPage({ onNavigate }: { onNavigate: (page: IosSettingsPage) => void }) {
  const { t } = useTranslation();
  const { themeMode, setThemeMode } = useTheme();
  const { preference, setLanguage } = useAppLanguage();
  const cloud = useCloudStore(
    useShallow((s) => ({
      status: s.status,
      busy: s.busy,
      hydrate: s.hydrate,
      signIn: s.signIn,
      signOut: s.signOut,
    }))
  );
  useEffect(() => {
    void cloud.hydrate();
  }, [cloud.hydrate]);

  const handleCloudSignIn = () => {
    if (cloud.busy) return;
    void cloud
      .signIn()
      .then(() =>
        Alert.alert(
          t('settings:live.cloudSignInSuccessTitle'),
          t('settings:live.cloudSignInSuccessBody')
        )
      )
      .catch((error) =>
        Alert.alert(
          t('settings:live.cloudSignInFailed'),
          error instanceof Error ? error.message : undefined
        )
      );
  };

  const handleCloudSignOut = () => {
    if (cloud.busy) return;
    void cloud.signOut().catch((error) =>
      Alert.alert(
        t('settingsStorage:setFailed'),
        error instanceof Error ? error.message : undefined
      )
    );
  };
  const state = useEngineStore(
    useShallow((s) => ({
      settings: s.snapshot?.settings.value,
      autoApplyReceived: s.autoApplyReceived,
      setAutoApplyReceived: s.setAutoApplyReceived,
      updateSettings: s.updateSettings,
    }))
  );

  const run = (promise: Promise<unknown>) => {
    void promise.catch((error) =>
      Alert.alert(
        t('settingsStorage:setFailed'),
        error instanceof Error ? error.message : undefined
      )
    );
  };

  const themeOptions = (['auto', 'light', 'dark'] as const).map((value) => ({
    value,
    label: t(`settings:appearance.mode.${value === 'auto' ? 'system' : value}`),
  }));

  const languageOptions = [
    { value: 'system', label: t('common:language.system') },
    { value: 'zh-CN', label: '简体中文' },
    { value: 'en', label: 'English' },
    { value: 'ru', label: 'Русский' },
    { value: 'pt-BR', label: 'Português' },
  ];

  const maxHistoryItems = state.settings?.history.max_items ?? 0;

  return (
    <IosSheetPage title={t('common:action.settings')}>
      <IosSheetForm>
        {/* ── UniClipboard Cloud 账号 ── */}
        <Section
          header={<SwiftUIText>{t('settings:live.cloudSection')}</SwiftUIText>}
          footer={<SwiftUIText>{t('settings:live.cloudFooter')}</SwiftUIText>}
        >
          {cloud.status?.signed_in ? (
            <>
              <SettingsNavRow
                testID="cloud-account"
                icon="icloud"
                iconColor={settingsTileColors.blue}
                title={cloudStatusLabel(cloud.status)}
                value={t('settings:live.cloudSignedIn')}
                readOnly
              />
              <SettingsNavRow
                testID="cloud-sign-out"
                title={t('settings:live.cloudSignOut')}
                destructive
                showsChevron={false}
                disabled={cloud.busy}
                onPress={handleCloudSignOut}
              />
            </>
          ) : cloud.status?.reauth_required ? (
            <>
              <SettingsNavRow
                testID="cloud-account"
                icon="icloud"
                iconColor={settingsTileColors.orange}
                title={cloudStatusLabel(cloud.status)}
                value={t('settings:live.cloudReauthRequired')}
                valueColor={statusOrange}
                readOnly
              />
              <SettingsNavRow
                testID="cloud-sign-in"
                title={cloud.busy ? t('settings:live.cloudSigningIn') : t('settings:live.cloudReauthAction')}
                showsChevron={!cloud.busy}
                disabled={cloud.busy}
                onPress={handleCloudSignIn}
              />
              <SettingsNavRow
                testID="cloud-sign-out"
                title={t('settings:live.cloudSignOut')}
                destructive
                showsChevron={false}
                disabled={cloud.busy}
                onPress={handleCloudSignOut}
              />
            </>
          ) : (
            <SettingsNavRow
              testID="cloud-sign-in"
              icon="icloud"
              iconColor={settingsTileColors.blue}
              title={cloud.busy ? t('settings:live.cloudSigningIn') : t('settings:live.cloudSignIn')}
              showsChevron={!cloud.busy}
              disabled={cloud.busy}
              onPress={handleCloudSignIn}
            />
          )}
        </Section>

        {/* ── 剪贴板同步（对齐 v1 第一组：内联自动写入/自动推送开关 + 更多同步设置） ── */}
        <Section
          header={<SwiftUIText>{t('settings:hub.clipboardSync.title')}</SwiftUIText>}
          footer={<SwiftUIText>{t('settings:ios.sync.footer')}</SwiftUIText>}
        >
          <IconToggleRow
            testID="settings-auto-apply"
            icon="arrow.down.doc"
            iconColor={settingsTileColors.green}
            label={t('settings:hub.clipboardSync.autoApply.title')}
            description={t('settings:hub.clipboardSync.autoApply.descP2p')}
            isOn={state.autoApplyReceived}
            onIsOnChange={(v) => run(state.setAutoApplyReceived(v))}
          />
          {state.settings ? (
            <IconToggleRow
              testID="settings-auto-push"
              icon="arrow.up.doc"
              iconColor={settingsTileColors.teal}
              label={t('settings:hub.clipboardSync.autoPush.title')}
              description={t('settings:hub.clipboardSync.autoPush.descP2p')}
              isOn={state.settings.sync.automatic}
              onIsOnChange={(v) =>
                run(
                  state.updateSettings((old) => ({
                    ...old,
                    sync: { ...old.sync, automatic: v },
                  }))
                )
              }
            />
          ) : null}
          <SettingsNavRow
            testID="sync"
            icon="arrow.triangle.2.circlepath"
            iconColor={settingsTileColors.blue}
            title={t('settings:category.sync')}
            onPress={() => onNavigate('sync')}
          />
        </Section>

        {/* ── 通用（对齐 v1 第二组：历史 / 剪贴板访问 / 主题 / 语言 / 存储 / 高级网络） ── */}
        <Section header={<SwiftUIText>{t('settings:general.sectionTitle')}</SwiftUIText>}>
          <SettingsNavRow
            testID="history"
            icon="clock.arrow.circlepath"
            iconColor={settingsTileColors.blue}
            title={t('settings:category.history')}
            value={
              maxHistoryItems > 0 ? maxHistoryItems.toLocaleString() : t('settings:live.unlimited')
            }
            onPress={() => onNavigate('history')}
          />
          <SettingsNavRow
            testID="clipboard"
            icon="doc.on.clipboard"
            iconColor={settingsTileColors.orange}
            title={t('settings:ios.extensions.clipboardAccess')}
            onPress={() => onNavigate('clipboard')}
          />
          <SettingsPickerRow
            testID="appearance"
            icon="circle.lefthalf.filled"
            iconColor={settingsTileColors.indigo}
            title={t('settings:general.theme')}
            options={themeOptions}
            selection={themeMode}
            onSelectionChange={(value) => void setThemeMode(value as typeof themeMode)}
          />
          <SettingsPickerRow
            testID="language"
            icon="globe"
            iconColor={settingsTileColors.blue}
            title={t('common:language.title')}
            options={languageOptions}
            selection={preference}
            onSelectionChange={(value) => void setLanguage(value as typeof preference)}
          />
          <SettingsNavRow
            testID="storage"
            icon="externaldrive"
            iconColor={settingsTileColors.purple}
            title={t('settings:category.storage')}
            onPress={() => onNavigate('storage')}
          />
          <SettingsNavRow
            testID="network"
            icon="network"
            iconColor={settingsTileColors.teal}
            title={t('settings:category.network')}
            onPress={() => onNavigate('network')}
          />
        </Section>

        {/* ── 扩展与权限（键盘扩展 / 分享扩展） ── */}
        <Section
          header={<SwiftUIText>{t('settings:category.extensions')}</SwiftUIText>}
          footer={<SwiftUIText>{t('settings:ios.extensions.footer')}</SwiftUIText>}
        >
          <SettingsNavRow
            testID="keyboard"
            icon="keyboard"
            iconColor={settingsTileColors.indigo}
            title={t('settings:ios.extensions.keyboard')}
            onPress={() => onNavigate('keyboard')}
          />
          <SettingsNavRow
            testID="share"
            icon="square.and.arrow.up"
            iconColor={settingsTileColors.green}
            title={t('settings:ios.extensions.share')}
            onPress={() => onNavigate('share')}
          />
        </Section>

        {/* ── 支持（对齐 v1 第四组：诊断日志） ── */}
        <Section header={<SwiftUIText>{t('settings:category.support')}</SwiftUIText>}>
          <SettingsNavRow
            testID="diagnostics"
            icon="waveform.path.ecg"
            iconColor={settingsTileColors.red}
            title={t('settings:category.diagnostics')}
            onPress={() => onNavigate('diagnostics')}
          />
        </Section>

        {/* ── 其他（对齐 v1 第五组：隐私 / 关于 + 版本号） ── */}
        <Section header={<SwiftUIText>{t('settings:category.other')}</SwiftUIText>}>
          <SettingsNavRow
            testID="privacy"
            icon="hand.raised.fill"
            iconColor={settingsTileColors.blue}
            title={t('settings:category.privacy')}
            onPress={() => onNavigate('privacy')}
          />
          <SettingsNavRow
            testID="about"
            icon="info.circle"
            iconColor={settingsTileColors.gray}
            title={t('settings:category.about')}
            value={nativeApplicationVersion ?? '—'}
            onPress={() => onNavigate('about')}
          />
        </Section>
      </IosSheetForm>
    </IosSheetPage>
  );
}
