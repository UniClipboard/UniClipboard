import { useEffect, useRef, useState } from 'react';
import { Alert } from 'react-native';
import { Section, Text as SwiftUIText } from '@expo/ui/swift-ui';
import { useTranslation } from 'react-i18next';

import { IosSheetForm, IosSheetPage } from '@/components/ui';
import { type TelemetryState, unicEngineNative } from '@/engine/nativeModule';
import {
  type DiagnosticWarningKey,
  mapDiagnosticWarningKeys,
  useDiagnosticCapture,
} from '../useDiagnosticCapture';
import { SettingsNavRow, SettingsToggle } from './common';

/**
 * 诊断日志页（iOS）：支持 10 分钟本地详细诊断录制与 ZIP 诊断包导出，
 * 保留 E2E 所需的 diagnostics-telemetry / diagnostics-export testID。
 */
export function DiagnosticsPage() {
  const { t } = useTranslation();
  const [state, setState] = useState<TelemetryState>();
  const [exporting, setExporting] = useState(false);
  const [warningKeys, setWarningKeys] = useState<DiagnosticWarningKey[]>([]);
  const changing = useRef(false);
  const capture = useDiagnosticCapture();

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

  const exportArchive = async () => {
    if (exporting) return;
    setExporting(true);
    setWarningKeys([]);
    try {
      const result = await unicEngineNative.exportDiagnosticArchive('share');
      setWarningKeys(mapDiagnosticWarningKeys(result.warnings));
    } catch {
      Alert.alert(
        t('settingsIos:diagnostics.error.title'),
        t('settingsIos:diagnostics.error.message')
      );
    } finally {
      setExporting(false);
    }
  };

  const captureSubtitle = capture.failed
    ? t('settingsIos:diagnostics.capture.failed')
    : !capture.available
      ? t('settingsIos:diagnostics.capture.unavailable')
      : capture.active
        ? t('settingsIos:diagnostics.capture.remaining', { count: capture.remainingMinutes })
        : undefined;

  const exportFooter = [
    t('settings:live.sessionLogsFooter'),
    ...warningKeys.map((key) => t(`settingsIos:diagnostics.warning.${key}`)),
  ].join('\n');

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
          header={<SwiftUIText>{t('settingsIos:diagnostics.capture.title')}</SwiftUIText>}
          footer={<SwiftUIText>{t('settingsIos:diagnostics.capture.description')}</SwiftUIText>}
        >
          <SettingsNavRow
            testID="diagnostics-capture"
            icon="waveform.path"
            title={
              capture.active
                ? t('settingsIos:diagnostics.capture.stop')
                : t('settingsIos:diagnostics.capture.start')
            }
            subtitle={captureSubtitle}
            disabled={!capture.available || capture.busy}
            showsChevron={false}
            onPress={() => void capture.toggle()}
          />
        </Section>

        <Section
          header={<SwiftUIText>{t('settingsAbout:log.title')}</SwiftUIText>}
          footer={<SwiftUIText>{exportFooter}</SwiftUIText>}
        >
          <SettingsNavRow
            testID="diagnostics-export"
            icon="square.and.arrow.up"
            title={
              exporting
                ? t('settingsIos:diagnostics.action.preparing')
                : t('settings:mobileDiagnostics.export')
            }
            subtitle={t('settings:live.sessionLogsSummary')}
            disabled={exporting}
            showsChevron={false}
            onPress={() => void exportArchive()}
          />
        </Section>
      </IosSheetForm>
    </IosSheetPage>
  );
}
