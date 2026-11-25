/**
 * 隐私二级页(Android) — 对齐原仓库 UniClip AnalyticsConsentControl.android.tsx
 * 与 iOS PrivacyPage.tsx，接入 v2 Engine 遥测偏好。
 */
import { memo, useEffect, useRef, useState } from 'react';
import { Alert } from 'react-native';
import { useTranslation } from 'react-i18next';

import { type TelemetryState, unicEngineNative } from '@/engine/nativeModule';
import { SettingsSectionItem } from '../SettingsSectionItem';
import { SettingsSwitchRow } from './SettingsSwitchRow';

export const PrivacySection = memo(function PrivacySection() {
  const { t } = useTranslation('settings');
  const [state, setState] = useState<TelemetryState>();
  const changing = useRef(false);

  useEffect(() => {
    let live = true;
    void unicEngineNative.getTelemetryState().then(
      (value) => {
        if (live) setState(value);
      },
      () => {
        if (live) Alert.alert(t('analytics.error'));
      }
    );
    return () => {
      live = false;
    };
  }, [t]);

  const change = async (enabled: boolean) => {
    if (changing.current) return;
    changing.current = true;
    try {
      setState(await unicEngineNative.setTelemetryEnabled(enabled));
    } catch {
      Alert.alert(t('analytics.error'));
    } finally {
      changing.current = false;
    }
  };

  const footer =
    state?.configured_enabled && !state.effective_enabled
      ? `${t('mobileDiagnostics.unavailable')}\n${t('analytics.footer')}`
      : t('analytics.footer');

  return (
    <SettingsSectionItem variant="grouped" footer={footer}>
      {state ? (
        <SettingsSwitchRow
          key="privacy-telemetry"
          testID="privacy-telemetry"
          title={t('analytics.consentTitle')}
          description={t('analytics.consentDescription')}
          value={state.configured_enabled}
          onValueChange={(enabled) => void change(enabled)}
        />
      ) : null}
    </SettingsSectionItem>
  );
});
