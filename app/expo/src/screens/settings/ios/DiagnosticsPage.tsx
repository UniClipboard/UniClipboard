import { useEffect, useRef, useState } from 'react';
import { Alert } from 'react-native';
import { Section, Text as SwiftUIText } from '@expo/ui/swift-ui';
import { useTranslation } from 'react-i18next';

import { IosSheetForm, IosSheetPage } from '@/components/ui';
import { type TelemetryState, unicEngineNative } from '@/engine/nativeModule';
import { SettingsNavRow, SettingsToggle } from './common';

/**
 * 诊断日志页（无重复分组标题、如实描述本次运行脱敏日志、保留 E2E 所需的 diagnostics-telemetry / diagnostics-export ID）。
 */
export function DiagnosticsPage() {
  const { t } = useTranslation();
  const [state, setState] = useState<TelemetryState>();
  const [exporting, setExporting] = useState(false);
  const changing = useRef(false);

  useEffect(() => {
    let live = true;
    void unicEngineNative.getTelemetryState().then(
      (value) => {
        if (live) setState(value);
      },
      () => {
        if (live) Alert.alert(t('settings:mobileDiagnostics.failed'));
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
      Alert.alert(t('settings:mobileDiagnostics.failed'));
    } finally {
      changing.current = false;
    }
  };

  const exportLogs = async () => {
    if (exporting) return;
    setExporting(true);
    try {
      const logs = await unicEngineNative.exportRecentDiagnosticLogs();
      if (logs) await unicEngineNative.shareHistoryMedia([], logs);
      else Alert.alert(t('settings:mobileDiagnostics.empty'));
    } catch {
      Alert.alert(t('settings:mobileDiagnostics.failed'));
    } finally {
      setExporting(false);
    }
  };

  return (
    <IosSheetPage title={t('settings:category.diagnostics')}>
      <IosSheetForm>
        <Section
          footer={
            state?.configured_enabled && !state.effective_enabled ? (
              <SwiftUIText>{t('settings:mobileDiagnostics.unavailable')}</SwiftUIText>
            ) : (
              <SwiftUIText>{t('settings:analytics.footer')}</SwiftUIText>
            )
          }
        >
          {state ? (
            <SettingsToggle
              testID="diagnostics-telemetry"
              label={t('settings:mobileDiagnostics.telemetry')}
              isOn={state.configured_enabled}
              onIsOnChange={(enabled) => void change(enabled)}
            />
          ) : null}
        </Section>

        <Section
          header={<SwiftUIText>{t('settingsAbout:log.title')}</SwiftUIText>}
          footer={<SwiftUIText>{t('settings:live.sessionLogsFooter')}</SwiftUIText>}
        >
          <SettingsNavRow
            testID="diagnostics-export"
            title={
              exporting
                ? t('settingsIos:diagnostics.action.preparing')
                : t('settings:mobileDiagnostics.export')
            }
            subtitle={t('settings:live.sessionLogsSummary')}
            disabled={exporting}
            showsChevron={false}
            onPress={() => void exportLogs()}
          />
        </Section>
      </IosSheetForm>
    </IosSheetPage>
  );
}
