import { useCallback, useEffect, useState } from 'react';
import { AppState, Linking, PermissionsAndroid, StyleSheet, Text, View } from 'react-native';
import { useTranslation } from 'react-i18next';
import { AppBottomSheet, AppButton, AppHost } from '@/components/ui';
import { unicEngineNative } from '@/engine/nativeModule';
import { useEngineStore } from '@/engine/store';
import { useTheme } from '@/hooks/useTheme';
import { showUnavailable } from '@/shell/notice';
import type { PlatformCapabilitiesController } from './usePlatformCapabilities.types';

export function usePlatformCapabilities(active: boolean): PlatformCapabilitiesController {
  const { t } = useTranslation();
  const { theme } = useTheme();
  const c = theme.colors;
  const mode = useEngineStore((s) => s.mode);

  const [notifyGranted, setNotifyGranted] = useState(false);
  const [batteryExempt, setBatteryExempt] = useState(false);
  const [bgInfoOpen, setBgInfoOpen] = useState(false);

  const refreshStatus = useCallback(async () => {
    try {
      const notifPerm = PermissionsAndroid.PERMISSIONS.POST_NOTIFICATIONS;
      if (notifPerm) {
        const granted = await PermissionsAndroid.check(notifPerm);
        setNotifyGranted(granted);
      } else {
        setNotifyGranted(true);
      }
    } catch {
      // ignore permission check failure on older SDKs
    }

    if (mode !== 'live') return;
    try {
      const status = await unicEngineNative.getBackgroundSyncStatus();
      setNotifyGranted(status.notification_permission_granted);
      setBatteryExempt(status.ignoring_battery_optimizations);
    } catch {
      // Keep last known background sync status.
    }
  }, [mode]);

  useEffect(() => {
    if (!active) return;
    void refreshStatus();
    const sub = AppState.addEventListener('change', (state) => {
      if (state === 'active') void refreshStatus();
    });
    return () => sub.remove();
  }, [active, refreshStatus]);

  const requestNotifications = useCallback(async () => {
    if (mode !== 'live') {
      showUnavailable();
      return;
    }
    const notifPerm = PermissionsAndroid.PERMISSIONS.POST_NOTIFICATIONS;
    if (notifPerm) {
      const result = await PermissionsAndroid.request(notifPerm);
      if (result === PermissionsAndroid.RESULTS.GRANTED) {
        setNotifyGranted(true);
        void unicEngineNative.setBackgroundSyncEnabled(true).catch(() => {});
      } else if (result === PermissionsAndroid.RESULTS.NEVER_ASK_AGAIN) {
        void Linking.openSettings();
      }
    } else {
      setNotifyGranted(true);
      void unicEngineNative.setBackgroundSyncEnabled(true).catch(() => {});
    }
    void refreshStatus();
  }, [mode, refreshStatus]);

  const requestBatteryExemption = useCallback(async () => {
    if (mode !== 'live') {
      showUnavailable();
      return;
    }
    const launched = await unicEngineNative.requestIgnoreBatteryOptimizations().catch(() => false);
    if (!launched) {
      void Linking.openSettings();
    }
    void refreshStatus();
  }, [mode, refreshStatus]);

  const rows: PlatformCapabilitiesController['rows'] = [
    {
      id: 'android-notify',
      icon: 'bell',
      iosTileColor: '#6750A4',
      title: t('onboarding:guide.caps.androidNotifyTitle'),
      subtitle: t('onboarding:guide.caps.androidNotifySub'),
      actionLabel: notifyGranted
        ? t('onboarding:guide.caps.androidNotifyDone')
        : t('onboarding:guide.caps.androidNotifyAllow'),
      actionTone: notifyGranted ? 'ok' : 'primary',
      onPress: () => void requestNotifications(),
      testID: 'onboarding-cap-android-notify',
    },
    {
      id: 'android-battery',
      icon: 'battery',
      iosTileColor: '#146C2E',
      title: t('onboarding:guide.caps.androidBatteryTitle'),
      subtitle: t('onboarding:guide.caps.androidBatterySub'),
      actionLabel: batteryExempt
        ? t('onboarding:guide.caps.androidBatteryDone')
        : t('onboarding:guide.caps.androidBatteryAction'),
      actionTone: batteryExempt ? 'ok' : 'soft',
      onPress: () => void requestBatteryExemption(),
      testID: 'onboarding-cap-android-battery',
    },
    {
      id: 'android-share',
      icon: 'share',
      iosTileColor: '#0B57D0',
      title: t('onboarding:guide.caps.androidShareTitle'),
      subtitle: t('onboarding:guide.caps.androidShareSub'),
      actionLabel: t('onboarding:guide.caps.androidShareInfo'),
      actionTone: 'soft',
      onPress: () => setBgInfoOpen(true),
      testID: 'onboarding-cap-android-share',
    },
  ];

  const readyItems: PlatformCapabilitiesController['readyItems'] = [
    {
      id: 'android-notify',
      icon: 'bell',
      iosTileColor: '#6750A4',
      title: t('onboarding:guide.ready.bgNotify'),
      statusLabel: notifyGranted
        ? t('onboarding:guide.ready.allowed')
        : t('onboarding:guide.ready.notEnabled'),
      isPositive: notifyGranted,
    },
    {
      id: 'android-battery',
      icon: 'battery',
      iosTileColor: '#146C2E',
      title: t('onboarding:guide.ready.batteryExempt'),
      statusLabel: batteryExempt
        ? t('onboarding:guide.ready.enabled')
        : t('onboarding:guide.ready.notEnabled'),
      isPositive: batteryExempt,
    },
  ];

  const renderOverlays = () => (
    <AppBottomSheet visible={bgInfoOpen} onDismiss={() => setBgInfoOpen(false)}>
      <View style={s.sheet}>
        <Text style={[s.sheetTitle, { color: c.textPrimary }]}>
          {t('onboarding:guide.caps.androidBgSheetTitle')}
        </Text>

        <View style={[s.stepCard, { backgroundColor: c.surfaceMid }]}>
          <View style={[s.stepNum, { backgroundColor: c.accent }]}>
            <Text style={[s.stepNumText, { color: c.onAccent }]}>1</Text>
          </View>
          <View style={s.stepBody}>
            <Text style={[s.stepTitle, { color: c.textPrimary }]}>
              {t('onboarding:guide.caps.androidBgStep1Title')}
            </Text>
            <Text style={[s.stepDesc, { color: c.textSecondary }]}>
              {t('onboarding:guide.caps.androidBgStep1Desc')}
            </Text>
          </View>
        </View>

        <View style={[s.stepCard, { backgroundColor: c.surfaceMid }]}>
          <View style={[s.stepNum, { backgroundColor: c.accent }]}>
            <Text style={[s.stepNumText, { color: c.onAccent }]}>2</Text>
          </View>
          <View style={s.stepBody}>
            <Text style={[s.stepTitle, { color: c.textPrimary }]}>
              {t('onboarding:guide.caps.androidBgStep2Title')}
            </Text>
            <Text style={[s.stepDesc, { color: c.textSecondary }]}>
              {t('onboarding:guide.caps.androidBgStep2Desc')}
            </Text>
          </View>
        </View>

        <AppHost style={s.doneHost} matchContents={{ vertical: true }}>
          <AppButton
            title={t('onboarding:guide.done')}
            variant="filled"
            fullWidth
            size="large"
            colors={{ containerColor: c.accent, contentColor: c.onAccent }}
            onPress={() => setBgInfoOpen(false)}
          />
        </AppHost>
      </View>
    </AppBottomSheet>
  );

  return {
    stepLabel: t('onboarding:guide.caps.androidStepLabel'),
    title: t('onboarding:guide.caps.androidTitle'),
    subtitle: t('onboarding:guide.caps.androidSubtitle'),
    rows,
    readyItems,
    renderOverlays,
  };
}

const s = StyleSheet.create({
  sheet: {
    paddingHorizontal: 20,
    paddingBottom: 16,
    gap: 10,
  },
  sheetTitle: {
    fontSize: 19,
    fontWeight: '700',
    marginBottom: 4,
  },
  stepCard: {
    flexDirection: 'row',
    alignItems: 'flex-start',
    gap: 12,
    padding: 14,
    borderRadius: 16,
  },
  stepNum: {
    width: 24,
    height: 24,
    borderRadius: 12,
    alignItems: 'center',
    justifyContent: 'center',
    marginTop: 1,
  },
  stepNumText: { fontSize: 12, fontWeight: '700' },
  stepBody: { flex: 1 },
  stepTitle: { fontSize: 14.5, fontWeight: '600' },
  stepDesc: { fontSize: 13, lineHeight: 18.5, marginTop: 3 },
  doneHost: { minHeight: 52, marginTop: 6 },
});
