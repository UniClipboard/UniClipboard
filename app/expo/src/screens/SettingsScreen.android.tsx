/**
 * 设置页面(Android) — 对齐原仓库 UniClip M3 Expressive 设置中枢（SettingsScreen.android.tsx），
 * 接入 v2 Go Engine 设置与状态。
 *
 * 顶部为双向剪贴板同步开关（自动写入 / 自动推送）与同步进阶入口；
 * 其下为「通用 / 支持 / 其他」三组带 40dp 圆形图标 + 动态摘要的入口，
 * 主题与语言直接以整行下拉选择呈现，其余设置下沉到 SettingsSub 二级页。
 */
import { memo, useCallback, useEffect, useState } from 'react';
import { Alert, StyleSheet } from 'react-native';
import { useFocusEffect, useNavigation } from '@react-navigation/native';
import type { NativeStackNavigationProp } from '@react-navigation/native-stack';
import { Host, LazyColumn } from '@expo/ui/jetpack-compose';
import { fillMaxSize } from '@expo/ui/jetpack-compose/modifiers';
import { nativeApplicationVersion } from 'expo-application';
import { SafeAreaView, useSafeAreaInsets } from 'react-native-safe-area-context';
import { useTranslation } from 'react-i18next';
import { useShallow } from 'zustand/react/shallow';

import { useFloatingNavigationClearance } from '@/components/android/floatingNavigationClearance';
import { unicEngineNative } from '@/engine/nativeModule';
import { useEngineStore } from '@/engine/store';
import type { BackgroundSyncStatus } from '@/engine/types';
import { useTheme } from '@/hooks/useTheme';
import { useAppLanguage } from '@/i18n/useAppLanguage';
import {
  LANGUAGE_NATIVE_NAMES,
  SUPPORTED_LANGUAGES,
  type LanguagePreference,
} from '@/i18n/languages';
import { cloudUserLabel } from '@/lib/cloudAuth';
import type { RootStackParamList, SettingsSubSection } from '@/navigation/AppNavigator.types';
import { pushSettingsSub } from '@/navigation/settingsSubNavigation';
import { useCloudStore } from '@/stores/cloudStore';
import { MATERIAL_SEED_COLOR } from '@/theme/colors';
import type { ThemeMode } from '@/theme';
import { SettingsScreenContent } from './settings/SettingsScreenContent';
import { SettingsSectionItem } from './settings/SettingsSectionItem';
import { SettingsLeadingIcon } from './settings/android/SettingsLeadingIcon';
import { SettingsListRow } from './settings/android/SettingsListRow';
import { SettingsSelectRow } from './settings/android/SettingsSelectRow';
import { SettingsSwitchRow } from './settings/android/SettingsSwitchRow';

const ICONS = {
  account: require('../assets/icons/account_circle.xml'),
  autoApply: require('../assets/icons/file_download.xml'),
  autoPush: require('../assets/icons/file_upload.xml'),
  sync: require('../assets/icons/restart_alt.xml'),
  history: require('../assets/icons/history.xml'),
  background: require('../assets/icons/layers.xml'),
  clipboard: require('../assets/icons/content_paste.xml'),
  theme: require('../assets/icons/palette.xml'),
  language: require('../assets/icons/public.xml'),
  storage: require('../assets/icons/storage.xml'),
  network: require('../assets/icons/wifi.xml'),
  diagnostics: require('../assets/icons/description.xml'),
  privacy: require('../assets/icons/privacy_tip.xml'),
  about: require('../assets/icons/info.xml'),
};

const HubRow = memo(function HubRow({
  section,
  icon,
  label,
  summary,
  muted,
  onNavigate,
}: {
  section: SettingsSubSection;
  icon: number;
  label: string;
  summary?: string;
  muted?: boolean;
  onNavigate: (section: SettingsSubSection) => void;
}) {
  return (
    <SettingsListRow
      testID={section}
      title={label}
      description={summary}
      icon={icon}
      iconTone={muted ? 'muted' : 'primary'}
      trailing="chevron"
      onPress={() => onNavigate(section)}
    />
  );
});

export function SettingsScreen() {
  const { t } = useTranslation();
  const { theme, themeMode, setThemeMode } = useTheme();
  const { preference, setLanguage } = useAppLanguage();
  const insets = useSafeAreaInsets();
  const navClearance = useFloatingNavigationClearance();
  const navigation = useNavigation<NativeStackNavigationProp<RootStackParamList>>();

  const state = useEngineStore(
    useShallow((s) => ({
      mode: s.mode,
      snapshot: s.snapshot,
      capabilities: s.capabilities,
      devices: s.devices,
      autoApplyReceived: s.autoApplyReceived,
      setAutoApplyReceived: s.setAutoApplyReceived,
      updateSettings: s.updateSettings,
    }))
  );
  const [backgroundStatus, setBackgroundStatus] = useState<BackgroundSyncStatus | null>(null);
  const cloud = useCloudStore(
    useShallow((s) => ({
      session: s.session,
      busy: s.busy,
      hydrate: s.hydrate,
      signIn: s.signIn,
      signOut: s.signOut,
    }))
  );
  useEffect(() => {
    void cloud.hydrate();
  }, [cloud.hydrate]);

  useFocusEffect(
    useCallback(() => {
      if (state.mode !== 'live') return;
      let active = true;
      void unicEngineNative
        .getBackgroundSyncStatus()
        .then((next) => {
          if (active) setBackgroundStatus(next);
        })
        .catch(() => {
          if (active) setBackgroundStatus(null);
        });
      return () => {
        active = false;
      };
    }, [state.mode])
  );

  if (state.mode === 'unavailable') {
    return <SettingsScreenContent page="root" />;
  }

  const settings = state.snapshot?.settings.value;
  const run = (promise: Promise<unknown>) => {
    void promise.catch((error) =>
      Alert.alert(
        t('settingsStorage:setFailed'),
        error instanceof Error ? error.message : undefined
      )
    );
  };

  const handleNavigate = (section: SettingsSubSection) =>
    pushSettingsSub(navigation, { section });

  const maxHistoryItems = settings?.history.max_items ?? 0;
  const historySummary =
    maxHistoryItems > 0
      ? t('settings:hub.summary.history', { count: maxHistoryItems })
      : t('settings:live.unlimited');

  const backgroundSummary =
    backgroundStatus === null
      ? undefined
      : backgroundStatus.enabled && !backgroundStatus.temp_disabled
        ? `${t('settings:hub.summary.backgroundOn')} · ${t(
            `settingsBackground:advanced.clipboardAccess.method.${backgroundStatus.clipboard_access_method}`
          )}`
        : t('settings:hub.summary.backgroundOff');

  const bindings = state.devices
    .flatMap((device) => device.bindings ?? [])
    .filter((binding) => binding.instance_id === 'builtin.tailcat');
  const connectedCount = bindings.filter((binding) => binding.connection === 'connected').length;
  const runtimeStatus = state.snapshot?.runtime
    ? t(`settings:live.status.${state.snapshot.runtime}`)
    : undefined;
  const networkSummary = runtimeStatus
    ? `${runtimeStatus} · ${t('settings:live.connected', {
        connected: connectedCount,
        total: bindings.length,
      })}`
    : undefined;

  const clipboardSummary = state.capabilities
    ? `${t('settings:live.observe')}：${t(`settings:live.status.${state.capabilities.effective.observe}`)} · ${t('settings:live.apply')}：${t(`settings:live.status.${state.capabilities.effective.apply}`)}`
    : undefined;

  const syncSummary = settings
    ? `${settings.device_name || state.snapshot?.self.name || '—'}`
    : undefined;

  const versionText = nativeApplicationVersion ?? '0.0.0';

  const themeOptions: { label: string; value: ThemeMode }[] = [
    { label: t('settings:appearance.mode.system'), value: 'auto' },
    { label: t('settings:appearance.mode.light'), value: 'light' },
    { label: t('settings:appearance.mode.dark'), value: 'dark' },
  ];

  const languageOptions: { label: string; value: LanguagePreference }[] = [
    { label: t('common:language.system'), value: 'system' },
    ...SUPPORTED_LANGUAGES.map((code) => ({
      label: LANGUAGE_NATIVE_NAMES[code],
      value: code as LanguagePreference,
    })),
  ];

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
          contentPadding={{
            start: 16,
            end: 16,
            top: 8,
            bottom: insets.bottom + navClearance + 40,
          }}
          verticalArrangement={{ spacedBy: 16 }}
        >
          {/* ── UniClipboard Cloud 账号 ── */}
          <SettingsSectionItem
            variant="grouped"
            title={t('settings:live.cloudSection')}
            footer={t('settings:live.cloudFooter')}
          >
            {cloud.session ? (
              <>
                <SettingsListRow
                  key="cloud-account"
                  testID="cloud-account"
                  icon={ICONS.account}
                  title={cloudUserLabel(cloud.session.user)}
                  description={t('settings:live.cloudSignedIn')}
                  trailing="none"
                />
                <SettingsListRow
                  key="cloud-sign-out"
                  testID="cloud-sign-out"
                  title={t('settings:live.cloudSignOut')}
                  destructive
                  disabled={cloud.busy}
                  trailing="none"
                  onPress={() => run(cloud.signOut())}
                />
              </>
            ) : (
              <SettingsListRow
                key="cloud-sign-in"
                testID="cloud-sign-in"
                icon={ICONS.account}
                title={
                  cloud.busy ? t('settings:live.cloudSigningIn') : t('settings:live.cloudSignIn')
                }
                description={t('settings:live.cloudSignInDesc')}
                trailing={cloud.busy ? 'none' : 'chevron'}
                disabled={cloud.busy}
                onPress={() => {
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
                }}
              />
            )}
          </SettingsSectionItem>

          {/* ── 剪贴板同步（对齐原仓库第一组：双向开关 + 更多同步设置） ── */}
          <SettingsSectionItem
            variant="grouped"
            title={t('settings:hub.clipboardSync.title')}
            footer={t('settings:hub.clipboardSync.footer')}
          >
            <SettingsSwitchRow
              key="auto-apply"
              testID="auto-apply"
              title={t('settings:hub.clipboardSync.autoApply.title')}
              description={t('settings:hub.clipboardSync.autoApply.descP2p')}
              leading={<SettingsLeadingIcon source={ICONS.autoApply} />}
              value={state.autoApplyReceived}
              onValueChange={(enabled) => run(state.setAutoApplyReceived(enabled))}
            />
            {settings ? (
              <SettingsSwitchRow
                key="auto-push"
                testID="auto-push"
                title={t('settings:hub.clipboardSync.autoPush.title')}
                description={t('settings:hub.clipboardSync.autoPush.descP2p')}
                leading={<SettingsLeadingIcon source={ICONS.autoPush} />}
                value={settings.sync.automatic}
                onValueChange={(enabled) =>
                  run(
                    state.updateSettings((old) => ({
                      ...old,
                      sync: { ...old.sync, automatic: enabled },
                    }))
                  )
                }
              />
            ) : null}
            <HubRow
              key="sync"
              section="sync"
              icon={ICONS.sync}
              label={t('settings:category.sync')}
              summary={syncSummary}
              onNavigate={handleNavigate}
            />
          </SettingsSectionItem>

          {/* ── 通用（对齐原仓库第二组：历史 / 后台 / 剪贴板权限 / 主题 / 语言 / 存储 / 高级网络） ── */}
          <SettingsSectionItem variant="grouped" title={t('settings:general.sectionTitle')}>
            <HubRow
              key="history"
              section="history"
              icon={ICONS.history}
              label={t('settings:category.history')}
              summary={historySummary}
              onNavigate={handleNavigate}
            />
            <HubRow
              key="background"
              section="background"
              icon={ICONS.background}
              label={t('settings:category.background')}
              summary={backgroundSummary}
              onNavigate={handleNavigate}
            />
            <HubRow
              key="clipboard"
              section="clipboard"
              icon={ICONS.clipboard}
              label={t('settings:ios.extensions.clipboardAccess')}
              summary={clipboardSummary}
              onNavigate={handleNavigate}
            />
            <SettingsSelectRow
              key="appearance"
              testID="appearance"
              icon={ICONS.theme}
              title={t('settings:general.theme')}
              description={t('settings:hub.summary.theme')}
              options={themeOptions}
              selectedValue={themeMode}
              onSelect={(mode) => void setThemeMode(mode)}
            />
            <SettingsSelectRow
              key="language"
              testID="language"
              icon={ICONS.language}
              title={t('common:language.title')}
              description={t('settings:hub.summary.language')}
              options={languageOptions}
              selectedValue={preference}
              onSelect={(pref) => void setLanguage(pref)}
            />
            <HubRow
              key="storage"
              section="storage"
              icon={ICONS.storage}
              label={t('settings:category.storage')}
              summary={t('settings:hub.summary.storage')}
              onNavigate={handleNavigate}
            />
            <HubRow
              key="network"
              section="network"
              icon={ICONS.network}
              label={t('settings:category.network')}
              summary={networkSummary}
              onNavigate={handleNavigate}
            />
          </SettingsSectionItem>

          {/* ── 支持（对齐原仓库第三组：诊断日志） ── */}
          <SettingsSectionItem variant="grouped" title={t('settings:category.support')}>
            <HubRow
              key="diagnostics"
              section="diagnostics"
              icon={ICONS.diagnostics}
              label={t('settings:category.diagnostics')}
              summary={t('settings:hub.summary.diagnostics')}
              onNavigate={handleNavigate}
            />
          </SettingsSectionItem>

          {/* ── 其他（对齐原仓库第四组：隐私 / 关于） ── */}
          <SettingsSectionItem variant="grouped" title={t('settings:category.other')}>
            <HubRow
              key="privacy"
              section="privacy"
              icon={ICONS.privacy}
              label={t('settings:category.privacy')}
              summary={t('settings:hub.summary.privacy')}
              onNavigate={handleNavigate}
            />
            <HubRow
              key="about"
              section="about"
              icon={ICONS.about}
              label={t('settings:category.about')}
              summary={t('settings:hub.summary.about', { version: versionText })}
              onNavigate={handleNavigate}
            />
          </SettingsSectionItem>
        </LazyColumn>
      </Host>
    </SafeAreaView>
  );
}

const styles = StyleSheet.create({
  container: { flex: 1 },
});
