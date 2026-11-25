/**
 * 后台运行二级页(Android) — 对齐原仓库 UniClip BackgroundSection.tsx M3 Expressive 设计：
 *
 * 顶部主开关使用 28dp SettingsHeroCard（开启时 primaryContainer 强调），开启时依次引导
 * 忽略电池优化 → 通知权限 → 当前后台剪贴板读取方式授权。
 * 其下三组 grouped 列表：
 * - 剪贴板读取：支持「定时轮询 / ADB 事件 / Shizuku」三种方式切换与引导弹窗
 * - 后台同步：后台下载远程 / 后台上传本地 / 移动数据下继续同步
 * - 运行保障：常驻通知 / 忽略电池优化 / 系统设置入口
 */
import { memo, useCallback, useEffect, useRef, useState } from 'react';
import { AppState, Linking, PermissionsAndroid } from 'react-native';
import { useFocusEffect } from '@react-navigation/native';
import {
  AlertDialog,
  Column,
  Spacer,
  Text as ComposeText,
  TextButton,
} from '@expo/ui/jetpack-compose';
import { fillMaxWidth, height as heightModifier } from '@expo/ui/jetpack-compose/modifiers';
import { useTranslation } from 'react-i18next';
import { useShallow } from 'zustand/react/shallow';

import { parseEngineError, unicEngineNative } from '@/engine/nativeModule';
import { useEngineStore } from '@/engine/store';
import type {
  BackgroundSyncConfigPatch,
  BackgroundSyncStatus,
  ClipboardAccessMethod,
} from '@/engine/types';
import { useMessageStore } from '@/stores/messageStore';
import {
  continueBackgroundClipboardSetup,
  getAdbGrantCommand,
  getBackgroundClipboardSetupState,
  SHIZUKU_START_COMMAND,
  type BackgroundClipboardSetupActionResult,
  type BackgroundClipboardSetupIssue,
} from '@/utils/backgroundClipboardAccess';
import { useClipboardAccessMethodSheet } from '../ClipboardAccessMethodSheet';
import { resolveAdbAuthorizationCheck } from '../ClipboardAccessMethodSheet.state';
import { SettingsSectionItem } from '../SettingsSectionItem';
import { SettingsHeroCard } from './SettingsHeroCard';
import { SettingsListRow } from './SettingsListRow';
import { SettingsSwitchRow } from './SettingsSwitchRow';

const ICONS = {
  terminal: require('../../../assets/icons/terminal.xml'),
  error: require('../../../assets/icons/error.xml'),
  check: require('../../../assets/icons/check.xml'),
  settings: require('../../../assets/icons/settings.xml'),
};

interface BgDialog {
  title: string;
  text: string;
  confirmLabel: string;
  onConfirm: () => void;
  dismissLabel?: string;
}

/** 等待下一次回到前台（用于串联跳系统设置页的权限申请），超时兜底避免永久挂起。 */
function waitForNextActiveState(timeoutMs = 60_000): Promise<void> {
  return new Promise((resolve) => {
    let done = false;
    const finish = () => {
      if (done) return;
      done = true;
      sub.remove();
      clearTimeout(timer);
      resolve();
    };
    const sub = AppState.addEventListener('change', (state) => {
      if (state === 'active') finish();
    });
    const timer = setTimeout(finish, timeoutMs);
  });
}

export const BackgroundSection = memo(function BackgroundSection() {
  const { t } = useTranslation('settingsBackground');
  const showMessage = useMessageStore((s) => s.showMessage);
  const {
    openMethodSheet,
    openAdbSetupSheet,
    openShizukuSetupSheet,
    openRestrictionSheet,
    closeSheet,
  } = useClipboardAccessMethodSheet();

  const { autoApplyReceived, autoPushLocal } = useEngineStore(
    useShallow((s) => ({
      autoApplyReceived: s.autoApplyReceived,
      autoPushLocal: (s.snapshot?.settings.value.sync.automatic ?? true) && s.automaticCapture,
    }))
  );

  const [status, setStatus] = useState<BackgroundSyncStatus | null>(null);
  const [dialog, setDialog] = useState<BgDialog | null>(null);
  const [adbCommandCopied, setAdbCommandCopied] = useState(false);
  const hasBatteryOptRequested = useRef(false);
  const statusRef = useRef<BackgroundSyncStatus | null>(null);
  statusRef.current = status;

  const refreshStatus = useCallback(async (): Promise<BackgroundSyncStatus | null> => {
    try {
      const next = await unicEngineNative.getBackgroundSyncStatus();
      setStatus(next);
      return next;
    } catch {
      return null;
    }
  }, []);

  useFocusEffect(
    useCallback(() => {
      void refreshStatus();
    }, [refreshStatus])
  );

  useEffect(() => {
    const appSub = AppState.addEventListener('change', (state) => {
      if (state === 'active') {
        void refreshStatus();
      }
    });
    const nativeSub = unicEngineNative.addBackgroundSyncStateListener(() => {
      void refreshStatus();
    });
    return () => {
      appSub.remove();
      nativeSub.remove();
    };
  }, [refreshStatus]);

  const applyConfig = useCallback(async (patch: BackgroundSyncConfigPatch) => {
    const next = await unicEngineNative.updateBackgroundSyncConfig(patch);
    setStatus(next);
    return next;
  }, []);

  const showError = useCallback(
    (error: unknown) => {
      const parsed = parseEngineError(error);
      showMessage(
        t(`errors:engine.${parsed.code}`, {
          defaultValue: t('toast.setFailed'),
        }),
        'error'
      );
    },
    [showMessage, t]
  );

  const openAdbSetupGuide = useCallback(
    (stage: 'instructions' | 'copied' | 'notDetected', currentStatus?: BackgroundSyncStatus) => {
      const target = currentStatus ?? statusRef.current;
      if (!target) return;
      openAdbSetupSheet({
        stage,
        command: getAdbGrantCommand(target),
        onCopy: () => void continueClipboardAccessSetup(),
        onCheck: () => void checkAdbAuthorization(),
      });
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [openAdbSetupSheet]
  );

  const handleClipboardAccessSetupResult = useCallback(
    async (
      result: BackgroundClipboardSetupActionResult,
      nextStatus?: BackgroundSyncStatus
    ) => {
      if (result === 'waiting-for-return') {
        await waitForNextActiveState();
      }
      if (result === 'failed') {
        showMessage(t('advanced.clipboardAccess.setup.failed'), 'error');
        return;
      }
      if (result === 'command-copied') {
        setAdbCommandCopied(true);
        showMessage(t('advanced.clipboardAccess.setup.commandCopied'), 'success');
        openAdbSetupGuide('copied', nextStatus);
      }
      if (result === 'completed') {
        showMessage(t('advanced.clipboardAccess.setup.completed'), 'success');
      }
      await refreshStatus();
    },
    [openAdbSetupGuide, refreshStatus, showMessage, t]
  );

  const continueClipboardAccessSetup = useCallback(async () => {
    try {
      const latest = (await refreshStatus()) ?? statusRef.current;
      if (!latest) return;
      const result = await continueBackgroundClipboardSetup(
        latest.clipboard_access_method,
        latest.authorization
      );
      await handleClipboardAccessSetupResult(result, latest);
    } catch (error: unknown) {
      showError(error);
    }
  }, [handleClipboardAccessSetupResult, refreshStatus, showError]);

  const checkAdbAuthorization = useCallback(async () => {
    try {
      const next = await refreshStatus();
      if (!next) return;
      const outcome = resolveAdbAuthorizationCheck(next.authorization);

      if (outcome === 'show-adb-guide') {
        openAdbSetupGuide('notDetected', next);
        return;
      }
      if (outcome === 'continue-access-setup') {
        setAdbCommandCopied(false);
        closeSheet();
        const result = await continueBackgroundClipboardSetup('overlay-event', next.authorization);
        await handleClipboardAccessSetupResult(result, next);
        return;
      }

      setAdbCommandCopied(false);
      closeSheet();
      showMessage(t('advanced.clipboardAccess.setup.completed'), 'success');
    } catch (error: unknown) {
      showError(error);
    }
  }, [closeSheet, handleClipboardAccessSetupResult, openAdbSetupGuide, refreshStatus, showError, showMessage, t]);

  const openRestrictionGuide = useCallback(() => {
    openRestrictionSheet({
      onConfirm: () => {
        closeSheet();
        void continueClipboardAccessSetup();
      },
    });
  }, [closeSheet, continueClipboardAccessSetup, openRestrictionSheet]);

  const openShizukuGuide = useCallback(
    (stage: 'notRunning' | 'authorize') => {
      openShizukuSetupSheet({
        stage,
        command: SHIZUKU_START_COMMAND,
        onAction: () => void runShizukuGuideAction(),
        onCheck: () => void checkShizukuSetup(),
      });
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [openShizukuSetupSheet]
  );

  const checkShizukuSetup = useCallback(async () => {
    try {
      const next = await refreshStatus();
      if (!next) return;
      const setup = getBackgroundClipboardSetupState(next.authorization);
      if (setup.issue === 'service-unavailable') return openShizukuGuide('notRunning');
      if (setup.issue === 'permission-required') return openShizukuGuide('authorize');
      closeSheet();
      if (setup.issue === 'system-restriction') return openRestrictionGuide();
      showMessage(t('advanced.clipboardAccess.setup.completed'), 'success');
    } catch (error: unknown) {
      showError(error);
    }
  }, [closeSheet, openRestrictionGuide, openShizukuGuide, refreshStatus, showError, showMessage, t]);

  const runShizukuGuideAction = useCallback(async () => {
    await continueClipboardAccessSetup();
    await checkShizukuSetup();
  }, [checkShizukuSetup, continueClipboardAccessSetup]);

  /** 开启总开关后依次引导：忽略电池优化 → 通知权限 → 当前读取方式授权。 */
  const runPermissionOnboarding = useCallback(async () => {
    let latest = await refreshStatus();
    if (!latest) return;

    if (!latest.ignoring_battery_optimizations) {
      const requested = await unicEngineNative.requestIgnoreBatteryOptimizations().catch(() => false);
      if (requested) {
        hasBatteryOptRequested.current = true;
        await waitForNextActiveState();
      }
    }

    try {
      const notifPerm = PermissionsAndroid.PERMISSIONS.POST_NOTIFICATIONS;
      if (notifPerm) {
        const hasNotif = await PermissionsAndroid.check(notifPerm);
        if (!hasNotif) {
          await PermissionsAndroid.request(notifPerm);
        }
      }
    } catch {
      // Android 12 及以下无需运行时通知权限
    }

    latest = await refreshStatus();
    if (!latest) return;
    if (
      latest.authorization.status === 'unavailable' ||
      latest.authorization.status === 'unauthorized'
    ) {
      const result = await continueBackgroundClipboardSetup(
        latest.clipboard_access_method,
        latest.authorization
      );
      if (result === 'waiting-for-return') {
        await waitForNextActiveState();
      }
    }
    await refreshStatus();
  }, [refreshStatus]);

  const handleToggleBackgroundTasks = async (enabled: boolean) => {
    if (!status) return;
    if (enabled) {
      if (status.temp_disabled) {
        try {
          await applyConfig({ enabled: true, temp_disabled: false });
          showMessage(t('toast.autoSyncRestored'), 'success');
        } catch (error: unknown) {
          showError(error);
        }
        return;
      }
      setDialog({
        title: t('dialog.enableAutoSync.title'),
        text: t('dialog.enableAutoSync.text'),
        confirmLabel: t('dialog.enableAutoSync.confirm'),
        dismissLabel: t('action.cancel', { ns: 'common' }),
        onConfirm: () => {
          void (async () => {
            try {
              await applyConfig({
                enabled: true,
                temp_disabled: false,
                background_download: true,
                background_upload: true,
                foreground_notification: true,
              });
              showMessage(t('toast.autoSyncEnabled'), 'success');
              void runPermissionOnboarding();
            } catch (error: unknown) {
              showError(error);
            }
          })();
        },
      });
      return;
    }

    try {
      await applyConfig({ enabled: false, temp_disabled: false });
      showMessage(t('toast.autoSyncDisabled'), 'success');
    } catch (error: unknown) {
      showError(error);
    }
  };

  const handleToggleForegroundNotification = async (enabled: boolean) => {
    if (!enabled) {
      setDialog({
        title: t('dialog.disableNotification.title'),
        text: t('dialog.disableNotification.text'),
        confirmLabel: t('dialog.disableNotification.confirm'),
        dismissLabel: t('action.cancel', { ns: 'common' }),
        onConfirm: () => {
          void applyConfig({ foreground_notification: false }).catch(showError);
        },
      });
      return;
    }

    try {
      await applyConfig({ foreground_notification: true });
      const notifPerm = PermissionsAndroid.PERMISSIONS.POST_NOTIFICATIONS;
      if (notifPerm) {
        const granted = await PermissionsAndroid.check(notifPerm);
        if (!granted) {
          setDialog({
            title: t('dialog.missingNotificationPermission.title'),
            text: t('dialog.missingNotificationPermission.text'),
            confirmLabel: t('dialog.missingNotificationPermission.confirm'),
            dismissLabel: t('dialog.missingNotificationPermission.dismiss'),
            onConfirm: () => void Linking.openSettings(),
          });
        }
      }
    } catch (error: unknown) {
      showError(error);
    }
  };

  const handleToggleBackgroundDownload = async (enabled: boolean) => {
    try {
      await applyConfig({ background_download: enabled });
      showMessage(enabled ? t('toast.downloadEnabled') : t('toast.downloadDisabled'), 'success');
    } catch (error: unknown) {
      showError(error);
    }
  };

  const handleToggleBackgroundUpload = async (enabled: boolean) => {
    if (enabled) {
      setDialog({
        title: t('dialog.enableUpload.title'),
        text: t('dialog.enableUpload.text'),
        confirmLabel: t('dialog.enableUpload.confirm'),
        dismissLabel: t('action.cancel', { ns: 'common' }),
        onConfirm: () => {
          void (async () => {
            try {
              await applyConfig({ background_upload: true });
              showMessage(t('toast.uploadEnabled'), 'success');
            } catch (error: unknown) {
              showError(error);
            }
          })();
        },
      });
      return;
    }

    try {
      await applyConfig({ background_upload: false });
      showMessage(t('toast.uploadDisabled'), 'success');
    } catch (error: unknown) {
      showError(error);
    }
  };

  const handleToggleCellularBackgroundSync = async (enabled: boolean) => {
    try {
      await applyConfig({ network: enabled ? 'any' : 'wifi' });
    } catch (error: unknown) {
      showError(error);
    }
  };

  const handleSetClipboardAccessMethod = async (method: ClipboardAccessMethod) => {
    if (!status || method === status.clipboard_access_method) return;
    try {
      setAdbCommandCopied(false);
      const next = await applyConfig({ clipboard_access_method: method });
      showMessage(t('toast.clipboardAccessChanged'), 'success');
      const setup = getBackgroundClipboardSetupState(next.authorization);
      if (setup.status === 'action-required') {
        if (method === 'overlay-event' && setup.issue === 'monitoring-setup-required') {
          openAdbSetupGuide('instructions', next);
          return;
        }
        if (method === 'shizuku') {
          if (setup.issue === 'service-unavailable') return openShizukuGuide('notRunning');
          if (setup.issue === 'permission-required') return openShizukuGuide('authorize');
          if (setup.issue === 'system-restriction') return openRestrictionGuide();
        }
        const result = await continueBackgroundClipboardSetup(method, next.authorization);
        await handleClipboardAccessSetupResult(result, next);
      }
    } catch (error: unknown) {
      showError(error);
    }
  };

  const handleOpenClipboardMethodSheet = () => {
    if (!status) return;
    openMethodSheet({
      selectedMethod: status.clipboard_access_method,
      onSelect: handleSetClipboardAccessMethod,
    });
  };

  const handleContinueClipboardAccessSetup = () => {
    if (!status) return;
    const setup = getBackgroundClipboardSetupState(status.authorization);
    const issue: BackgroundClipboardSetupIssue | null = setup.issue;
    if (issue === 'monitoring-setup-required') {
      openAdbSetupGuide(adbCommandCopied ? 'copied' : 'instructions', status);
      return;
    }
    if (issue === 'system-restriction') {
      openRestrictionGuide();
      return;
    }
    if (status.clipboard_access_method === 'shizuku') {
      if (issue === 'service-unavailable') return openShizukuGuide('notRunning');
      if (issue === 'permission-required') return openShizukuGuide('authorize');
    }
    void continueClipboardAccessSetup();
  };

  const handleToggleBattery = async () => {
    if (hasBatteryOptRequested.current) {
      setDialog({
        title: t('dialog.batteryFallback.title'),
        text: t('dialog.batteryFallback.text'),
        confirmLabel: t('dialog.batteryFallback.confirm'),
        dismissLabel: t('action.cancel', { ns: 'common' }),
        onConfirm: () => void Linking.openSettings(),
      });
      return;
    }
    const launched = await unicEngineNative.requestIgnoreBatteryOptimizations().catch(() => false);
    hasBatteryOptRequested.current = true;
    if (!launched) {
      setDialog({
        title: t('dialog.batteryFallback.title'),
        text: t('dialog.batteryFallback.text'),
        confirmLabel: t('dialog.batteryFallback.confirm'),
        dismissLabel: t('action.cancel', { ns: 'common' }),
        onConfirm: () => void Linking.openSettings(),
      });
    }
  };

  if (status === null) {
    return (
      <Column modifiers={[fillMaxWidth()]}>
        <SettingsHeroCard tone="neutral" contentPadding={8}>
          <SettingsListRow
            testID="background-sync-loading"
            title={t('live.loading', { ns: 'settings' })}
          />
        </SettingsHeroCard>
      </Column>
    );
  }

  const backgroundTasksEnabled = status.enabled && !status.temp_disabled;
  const method = status.clipboard_access_method;
  const clipboardSetupState = getBackgroundClipboardSetupState(status.authorization);

  return (
    <Column modifiers={[fillMaxWidth()]}>
      <SettingsHeroCard tone={backgroundTasksEnabled ? 'primary' : 'neutral'} contentPadding={8}>
        <SettingsSwitchRow
          testID="background-sync"
          title={t('main.cardTitle')}
          description={
            status.temp_disabled ? t('main.toggle.descTempDisabled') : t('main.toggle.descNormal')
          }
          value={backgroundTasksEnabled}
          onValueChange={(enabled) => void handleToggleBackgroundTasks(enabled)}
        />
      </SettingsHeroCard>

      <Spacer modifiers={[heightModifier(24)]} />
      <SettingsSectionItem
        variant="grouped"
        title={t('advanced.groups.clipboardAccess')}
        dialogs={
          dialog ? (
            <AlertDialog onDismissRequest={() => setDialog(null)}>
              <AlertDialog.Title>
                <ComposeText>{dialog.title}</ComposeText>
              </AlertDialog.Title>
              <AlertDialog.Text>
                <ComposeText>{dialog.text}</ComposeText>
              </AlertDialog.Text>
              <AlertDialog.ConfirmButton>
                <TextButton
                  onClick={() => {
                    const fn = dialog.onConfirm;
                    setDialog(null);
                    fn();
                  }}
                >
                  <ComposeText>{dialog.confirmLabel}</ComposeText>
                </TextButton>
              </AlertDialog.ConfirmButton>
              {dialog.dismissLabel ? (
                <AlertDialog.DismissButton>
                  <TextButton onClick={() => setDialog(null)}>
                    <ComposeText>{dialog.dismissLabel}</ComposeText>
                  </TextButton>
                </AlertDialog.DismissButton>
              ) : null}
            </AlertDialog>
          ) : null
        }
      >
        <SettingsListRow
          key="clipboardAccess"
          testID="background-clipboard-access"
          icon={ICONS.terminal}
          title={t('advanced.clipboardAccess.title')}
          description={t(`advanced.clipboardAccess.method.${method}`)}
          trailing={{
            action: t('advanced.clipboardAccess.entry.change'),
            chevron: true,
          }}
          onPress={handleOpenClipboardMethodSheet}
        />
        {clipboardSetupState.status === 'action-required' && clipboardSetupState.issue ? (
          <SettingsListRow
            key="clipboardAccessIssue"
            testID="background-clipboard-access-issue"
            tone="warning"
            icon={ICONS.error}
            title={t(`advanced.clipboardAccess.issue.${clipboardSetupState.issue}`)}
            description={t(
              `advanced.clipboardAccess.issueDescription.${clipboardSetupState.issue}`
            )}
            trailing={{
              pill: t(`advanced.clipboardAccess.action.${clipboardSetupState.issue}`),
            }}
            onPress={handleContinueClipboardAccessSetup}
          />
        ) : (
          <SettingsListRow
            key="clipboardAccessReady"
            testID="background-clipboard-access-ready"
            icon={ICONS.check}
            title={t('advanced.clipboardAccess.status.ready.title')}
            description={t(`advanced.clipboardAccess.status.ready.description.${method}`)}
            trailing="chevron"
            onPress={handleOpenClipboardMethodSheet}
          />
        )}
      </SettingsSectionItem>

      <Spacer modifiers={[heightModifier(24)]} />
      <SettingsSectionItem
        variant="grouped"
        title={t('advanced.groups.sync')}
        footer={t('main.footer')}
      >
        <SettingsSwitchRow
          key="download"
          testID="background-download"
          title={t('advanced.download.title')}
          description={t('advanced.download.desc')}
          value={backgroundTasksEnabled && autoApplyReceived && status.background_download}
          disabled={!(backgroundTasksEnabled && autoApplyReceived)}
          onValueChange={(enabled) => void handleToggleBackgroundDownload(enabled)}
        />
        <SettingsSwitchRow
          key="upload"
          testID="background-upload"
          title={t('advanced.upload.title')}
          description={t('advanced.upload.desc')}
          value={backgroundTasksEnabled && autoPushLocal && status.background_upload}
          disabled={!(backgroundTasksEnabled && autoPushLocal)}
          onValueChange={(enabled) => void handleToggleBackgroundUpload(enabled)}
        />
        <SettingsSwitchRow
          key="network"
          testID="background-network"
          title={t('advanced.network.title')}
          description={t('advanced.network.desc')}
          value={status.network === 'any'}
          disabled={!backgroundTasksEnabled}
          onValueChange={(enabled) => void handleToggleCellularBackgroundSync(enabled)}
        />
      </SettingsSectionItem>

      <Spacer modifiers={[heightModifier(24)]} />
      <SettingsSectionItem variant="grouped" title={t('advanced.groups.reliability')}>
        <SettingsSwitchRow
          key="notification"
          testID="background-notification"
          title={t('advanced.notification.title')}
          description={t('advanced.notification.desc')}
          value={backgroundTasksEnabled && status.foreground_notification}
          disabled={!backgroundTasksEnabled}
          onValueChange={(enabled) => void handleToggleForegroundNotification(enabled)}
        />
        <SettingsSwitchRow
          key="battery"
          testID="background-battery"
          title={t('advanced.battery.title')}
          description={t('advanced.battery.desc')}
          value={status.ignoring_battery_optimizations}
          onValueChange={() => void handleToggleBattery()}
        />
        <SettingsListRow
          key="background-system-settings"
          testID="background-system-settings"
          icon={ICONS.settings}
          iconTone="muted"
          title={t('openSystemSettings', { ns: 'settings' })}
          trailing="chevron"
          onPress={() => void Linking.openSettings()}
        />
      </SettingsSectionItem>
    </Column>
  );
});
