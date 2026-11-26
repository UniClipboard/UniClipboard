/**
 * 关于二级页(Android) — 对齐原仓库 UniClip AboutSection.tsx M3 Expressive 设计，
 * 包含应用信息卡片（像素猫品牌标 + 版本号 + 检查更新/下载安装 + GitHub 入口）、
 * 自动检查更新与测试渠道开关，以及渠道确认/下载源选择/取消下载弹窗。
 */
import { memo, useCallback, useEffect, useRef, useState } from 'react';
import {
  AlertDialog,
  Badge,
  Button,
  Column,
  FilledTonalButton,
  Icon,
  OutlinedButton,
  RNHostView,
  Row,
  Spacer,
  Text as ComposeText,
  TextButton,
  useMaterialColors,
} from '@expo/ui/jetpack-compose';
import {
  fillMaxWidth,
  height as heightModifier,
  size,
  testID,
  width as widthModifier,
} from '@expo/ui/jetpack-compose/modifiers';
import { useTranslation } from 'react-i18next';

import { UnicBrandLogo } from '@/components/UnicBrandLogo';
import { openWebLink } from '@/engine/historyActions';
import { unicEngineNative } from '@/engine/nativeModule';
import { EngineError } from '@/engine/types';
import { useMessageStore } from '@/stores/messageStore';
import {
  cancelAndroidUpdateDownload,
  checkForAndroidUpdate,
  cleanOldAndroidUpdateCache,
  downloadAndInstallAndroidUpdate,
  getInstalledAndroidIdentity,
  loadAndroidUpdateSettings,
  saveAutoCheckUpdate,
  saveUpdateChannel,
  type AndroidApkSource,
  type AndroidUpdateChannel,
  type AndroidUpdateCheckResult,
} from '@/updates/androidUpdater';
import { SettingsSectionItem } from '../SettingsSectionItem';
import { SettingsHeroCard } from './SettingsHeroCard';
import { SettingsListRow } from './SettingsListRow';
import { SettingsSwitchRow } from './SettingsSwitchRow';

const PROJECT_HOME = 'https://github.com/UniClipboard/UniClipboard';

const ICONS = {
  openInNew: require('../../../assets/icons/open_in_new.xml'),
  globe: require('../../../assets/icons/public.xml'),
};

const APP_ICON_SIZE = 84;
const APP_NAME_STYLE = { typography: 'titleLarge' } as const;

export const AboutSection = memo(function AboutSection() {
  const { t, i18n } = useTranslation();
  const colors = useMaterialColors();
  const showMessage = useMessageStore((s) => s.showMessage);
  const installed = getInstalledAndroidIdentity();
  const activeLanguage = i18n.resolvedLanguage ?? i18n.language ?? 'en';

  const [autoCheckEnabled, setAutoCheckEnabled] = useState(true);
  const [updateChannel, setUpdateChannel] = useState<AndroidUpdateChannel>('stable');
  const [isCheckingUpdate, setIsCheckingUpdate] = useState(false);
  const [isSavingChannel, setIsSavingChannel] = useState(false);
  const [availableUpdate, setAvailableUpdate] = useState<AndroidUpdateCheckResult | null>(null);
  const [isDownloading, setIsDownloading] = useState(false);
  const [downloadProgress, setDownloadProgress] = useState(0);

  const [showBetaConfirmation, setShowBetaConfirmation] = useState(false);
  const [downloadDialogUpdate, setDownloadDialogUpdate] = useState<AndroidUpdateCheckResult | null>(
    null
  );
  const [showCancelDownloadDialog, setShowCancelDownloadDialog] = useState(false);

  const checkGenerationRef = useRef(0);
  const savingChannelRef = useRef(false);

  useEffect(() => {
    let active = true;
    void loadAndroidUpdateSettings()
      .then((loaded) => {
        if (!active) return;
        setAutoCheckEnabled(loaded.autoCheckUpdate);
        setUpdateChannel(loaded.updateChannel);
      })
      .catch(() => {});
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    const subscription = unicEngineNative.addUpdateDownloadProgressListener(({ progress }) => {
      setDownloadProgress(progress);
    });
    return () => {
      subscription.remove();
    };
  }, []);

  const runUpdateCheck = useCallback(
    async (showNoUpdateToast: boolean, channelOverride?: AndroidUpdateChannel) => {
      const generation = ++checkGenerationRef.current;
      const targetChannel = channelOverride ?? updateChannel;
      setIsCheckingUpdate(true);
      try {
        const result = await checkForAndroidUpdate(targetChannel, activeLanguage);
        if (generation !== checkGenerationRef.current) return;
        if (result.hasUpdate) {
          setAvailableUpdate(result);
          setDownloadDialogUpdate(result);
          void cleanOldAndroidUpdateCache(result.versionKey).catch(() => {});
        } else {
          setAvailableUpdate(null);
          setDownloadDialogUpdate(null);
          void cleanOldAndroidUpdateCache(null).catch(() => {});
          if (showNoUpdateToast) {
            showMessage(t('settingsAbout:update.upToDate'), 'success');
          }
        }
      } catch {
        if (showNoUpdateToast && generation === checkGenerationRef.current) {
          showMessage(t('settingsAbout:update.checkFailed'), 'error');
        }
      } finally {
        if (generation === checkGenerationRef.current) {
          setIsCheckingUpdate(false);
        }
      }
    },
    [activeLanguage, showMessage, t, updateChannel]
  );

  const handleDownloadAndInstall = useCallback(
    async (update: AndroidUpdateCheckResult, source: AndroidApkSource) => {
      if (isDownloading) return;
      if (!update.artifact) {
        showMessage(t('settingsAbout:download.noSuitableApk'), 'error');
        return;
      }
      setIsDownloading(true);
      setDownloadProgress(0);
      try {
        const outcome = await downloadAndInstallAndroidUpdate(update, source);
        if (outcome.status === 'needs_install_permission') {
          showMessage(t('settingsAbout:download.needsInstallPermission'), 'info');
        }
      } catch (err) {
        if (err instanceof EngineError && err.code === 'cancelled') {
          showMessage(t('settingsAbout:download.canceled'), 'info');
        } else if (err instanceof Error && err.message === 'NO_SUITABLE_APK') {
          showMessage(t('settingsAbout:download.noSuitableApk'), 'error');
        } else {
          showMessage(t('settingsAbout:download.failed'), 'error');
        }
      } finally {
        setIsDownloading(false);
        setDownloadProgress(0);
      }
    },
    [isDownloading, showMessage, t]
  );

  const handleToggleAutoCheck = useCallback(
    async (enabled: boolean) => {
      setAutoCheckEnabled(enabled);
      try {
        await saveAutoCheckUpdate(enabled);
      } catch {
        setAutoCheckEnabled(!enabled);
        showMessage(t('settingsAbout:error.saveFailed'), 'error');
      }
    },
    [showMessage, t]
  );

  const applyChannelChange = useCallback(
    async (nextChannel: AndroidUpdateChannel) => {
      if (savingChannelRef.current) return;
      savingChannelRef.current = true;
      setIsSavingChannel(true);
      try {
        await saveUpdateChannel(nextChannel);
        setUpdateChannel(nextChannel);
        setShowBetaConfirmation(false);
        setAvailableUpdate(null);
        setDownloadDialogUpdate(null);
        if (nextChannel === 'alpha') {
          await runUpdateCheck(true, 'alpha');
        }
      } catch {
        showMessage(t('settingsAbout:error.saveFailed'), 'error');
      } finally {
        savingChannelRef.current = false;
        setIsSavingChannel(false);
      }
    },
    [runUpdateCheck, showMessage, t]
  );

  const handleToggleAlphaChannel = useCallback(
    (enabled: boolean) => {
      if (savingChannelRef.current || isCheckingUpdate || isDownloading) return;
      const isAlpha = updateChannel === 'alpha';
      if (enabled === isAlpha) return;
      checkGenerationRef.current += 1;
      if (enabled) {
        setShowBetaConfirmation(true);
      } else {
        void applyChannelChange('stable');
      }
    },
    [applyChannelChange, isCheckingUpdate, isDownloading, updateChannel]
  );

  const updateToBetaEnabled = updateChannel === 'alpha';

  return (
    <Column modifiers={[fillMaxWidth()]}>
      <SettingsHeroCard horizontalAlignment="center">
        <RNHostView matchContents modifiers={[size(APP_ICON_SIZE, APP_ICON_SIZE)]}>
          <UnicBrandLogo size={APP_ICON_SIZE} borderRadius={22} />
        </RNHostView>
        <Column horizontalAlignment="center">
          <ComposeText style={APP_NAME_STYLE} modifiers={[testID('about-name')]}>
            Unic
          </ComposeText>
          <ComposeText color={colors.onSurfaceVariant} modifiers={[testID('about-version')]}>
            {t('settingsAbout:versionValue', { version: installed.displayVersion })}
          </ComposeText>
        </Column>
        {availableUpdate ? (
          <Badge
            containerColor={colors.tertiaryContainer}
            contentColor={colors.onTertiaryContainer}
          >
            <ComposeText maxLines={1}>
              {t('settingsAbout:update.available', { version: availableUpdate.displayVersion })}
            </ComposeText>
          </Badge>
        ) : null}
        <Row verticalAlignment="center">
          {isDownloading || availableUpdate ? (
            <Button
              modifiers={[testID('about-update')]}
              enabled={!isCheckingUpdate}
              onClick={() => {
                if (isDownloading) {
                  setShowCancelDownloadDialog(true);
                } else if (availableUpdate) {
                  setDownloadDialogUpdate(availableUpdate);
                }
              }}
            >
              <ComposeText>
                {isDownloading
                  ? t('settingsAbout:download.downloading', {
                      percent: Math.round(downloadProgress * 100),
                    })
                  : t('settingsAbout:update.updateTo', {
                      version: availableUpdate?.displayVersion ?? '',
                    })}
              </ComposeText>
            </Button>
          ) : (
            <FilledTonalButton
              modifiers={[testID('about-check-update')]}
              enabled={!isCheckingUpdate}
              onClick={() => void runUpdateCheck(true)}
            >
              <ComposeText>
                {isCheckingUpdate
                  ? t('settingsAbout:update.checking')
                  : t('settingsAbout:update.check')}
              </ComposeText>
            </FilledTonalButton>
          )}
          <Spacer modifiers={[widthModifier(8)]} />
          <OutlinedButton
            modifiers={[testID('about-github')]}
            onClick={() => void openWebLink(PROJECT_HOME)}
          >
            <Icon source={ICONS.openInNew} size={18} tint={colors.primary} />
            <Spacer modifiers={[widthModifier(8)]} />
            <ComposeText>GitHub</ComposeText>
          </OutlinedButton>
        </Row>
      </SettingsHeroCard>

      <Spacer modifiers={[heightModifier(16)]} />

      <SettingsSectionItem
        variant="grouped"
        title={t('settingsAbout:updatesTitle')}
        dialogs={
          <>
            {showBetaConfirmation ? (
              <AlertDialog
                onDismissRequest={() => {
                  if (!savingChannelRef.current) setShowBetaConfirmation(false);
                }}
              >
                <AlertDialog.Title>
                  <ComposeText>{t('settingsAbout:updateToBeta.confirmTitle')}</ComposeText>
                </AlertDialog.Title>
                <AlertDialog.Text>
                  <Column verticalArrangement={{ spacedBy: 8 }}>
                    <ComposeText color={colors.onSurfaceVariant}>
                      {t('settingsAbout:updateToBeta.description')}
                    </ComposeText>
                    <ComposeText color={colors.error}>
                      {t('settingsAbout:updateToBeta.warning')}
                    </ComposeText>
                    <ComposeText color={colors.onSurfaceVariant}>
                      {t('settingsAbout:updateToBeta.backup')}
                    </ComposeText>
                  </Column>
                </AlertDialog.Text>
                <AlertDialog.ConfirmButton>
                  <TextButton
                    enabled={!isSavingChannel}
                    onClick={() => void applyChannelChange('alpha')}
                  >
                    <ComposeText>{t('settingsAbout:updateToBeta.confirm')}</ComposeText>
                  </TextButton>
                </AlertDialog.ConfirmButton>
                <AlertDialog.DismissButton>
                  <TextButton
                    enabled={!isSavingChannel}
                    onClick={() => setShowBetaConfirmation(false)}
                  >
                    <ComposeText>{t('settingsAbout:updateToBeta.cancel')}</ComposeText>
                  </TextButton>
                </AlertDialog.DismissButton>
              </AlertDialog>
            ) : null}

            {downloadDialogUpdate ? (
              <AlertDialog onDismissRequest={() => setDownloadDialogUpdate(null)}>
                <AlertDialog.Title>
                  <ComposeText>{t('settingsAbout:download.newVersionTitle')}</ComposeText>
                </AlertDialog.Title>
                <AlertDialog.Text>
                  <ComposeText>
                    {`${t('settingsAbout:download.latestVersion', {
                      version: downloadDialogUpdate.displayVersion,
                    })}\n${t('settingsAbout:download.currentVersion', {
                      version: installed.displayVersion,
                    })}${
                      downloadDialogUpdate.releaseNotes
                        ? `\n\n${t('settingsAbout:download.releaseNotes', {
                            notes: downloadDialogUpdate.releaseNotes,
                          })}`
                        : ''
                    }`}
                  </ComposeText>
                </AlertDialog.Text>
                <AlertDialog.ConfirmButton>
                  <TextButton
                    onClick={() => {
                      const target = downloadDialogUpdate;
                      setDownloadDialogUpdate(null);
                      void handleDownloadAndInstall(target, 'r2');
                    }}
                  >
                    <ComposeText>{t('settingsAbout:download.r2')}</ComposeText>
                  </TextButton>
                </AlertDialog.ConfirmButton>
                <AlertDialog.DismissButton>
                  <TextButton
                    onClick={() => {
                      const target = downloadDialogUpdate;
                      setDownloadDialogUpdate(null);
                      void handleDownloadAndInstall(target, 'github');
                    }}
                  >
                    <ComposeText>{t('settingsAbout:download.github')}</ComposeText>
                  </TextButton>
                </AlertDialog.DismissButton>
              </AlertDialog>
            ) : null}

            {showCancelDownloadDialog ? (
              <AlertDialog onDismissRequest={() => setShowCancelDownloadDialog(false)}>
                <AlertDialog.Title>
                  <ComposeText>{t('settingsAbout:cancelDownload.title')}</ComposeText>
                </AlertDialog.Title>
                <AlertDialog.Text>
                  <ComposeText>{t('settingsAbout:cancelDownload.message')}</ComposeText>
                </AlertDialog.Text>
                <AlertDialog.ConfirmButton>
                  <TextButton
                    onClick={() => {
                      setShowCancelDownloadDialog(false);
                      void cancelAndroidUpdateDownload();
                    }}
                  >
                    <ComposeText>{t('settingsAbout:cancelDownload.confirm')}</ComposeText>
                  </TextButton>
                </AlertDialog.ConfirmButton>
                <AlertDialog.DismissButton>
                  <TextButton onClick={() => setShowCancelDownloadDialog(false)}>
                    <ComposeText>{t('settingsAbout:cancelDownload.dismiss')}</ComposeText>
                  </TextButton>
                </AlertDialog.DismissButton>
              </AlertDialog>
            ) : null}
          </>
        }
      >
        <SettingsSwitchRow
          key="autoCheck"
          testID="about-auto-check"
          title={t('settingsAbout:autoCheck.label')}
          value={autoCheckEnabled}
          onValueChange={(enabled) => void handleToggleAutoCheck(enabled)}
        />
        <SettingsSwitchRow
          key="updateToBeta"
          testID="about-update-to-beta"
          title={t('settingsAbout:updateToBeta.label')}
          value={updateToBetaEnabled}
          disabled={isSavingChannel || isCheckingUpdate || isDownloading}
          onValueChange={handleToggleAlphaChannel}
        />
      </SettingsSectionItem>

      <Spacer modifiers={[heightModifier(16)]} />

      <SettingsSectionItem variant="grouped">
        <SettingsListRow
          key="project-home"
          testID="about-project-home"
          icon={ICONS.globe}
          iconTone="muted"
          title={t('settings:ios.about.projectHome')}
          description="UniClipboard/UniClipboard"
          trailing={{ text: 'GitHub', chevron: true }}
          onPress={() => void openWebLink(PROJECT_HOME)}
        />
      </SettingsSectionItem>
    </Column>
  );
});
