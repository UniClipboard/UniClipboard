import { useEffect, useRef, useState } from "react";
import { Alert } from "react-native";
import { useTranslation } from "react-i18next";
import { type TelemetryState, unicEngineNative } from "@/engine/nativeModule";
import { SettingsPage } from "./SettingsPage";
import { mapDiagnosticWarningKeys, useDiagnosticCapture } from "./useDiagnosticCapture";

export function DiagnosticsSettings({ title, onBack }: { title: string; onBack?: () => void }) {
  const { t } = useTranslation();
  const [state, setState] = useState<TelemetryState>();
  const [exporting, setExporting] = useState(false);
  const changing = useRef(false);
  const capture = useDiagnosticCapture();

  useEffect(() => {
    let live = true;
    void unicEngineNative.getTelemetryState().then((value) => {
      if (live) setState(value);
    }, () => {
      if (live) Alert.alert(t("settings:mobileDiagnostics.failed"));
    });
    return () => { live = false; };
  }, [t]);

  const change = async (enabled: boolean) => {
    if (changing.current) return;
    changing.current = true;
    try {
      setState(await unicEngineNative.setTelemetryEnabled(enabled));
    } catch {
      Alert.alert(t("settings:mobileDiagnostics.failed"));
    } finally {
      changing.current = false;
    }
  };

  const exportArchive = async () => {
    if (exporting) return;
    setExporting(true);
    try {
      const result = await unicEngineNative.exportDiagnosticArchive("share");
      const warnings = mapDiagnosticWarningKeys(result.warnings);
      if (warnings.length > 0) {
        Alert.alert(
          t("settingsAbout:log.title"),
          warnings.map((key) => t(`settingsAbout:log.warning.${key}`)).join("\n"),
        );
      }
    } catch {
      Alert.alert(t("settings:mobileDiagnostics.failed"));
    } finally {
      setExporting(false);
    }
  };

  return <SettingsPage title={title} onBack={onBack} groups={[
    {
      footer: state?.configured_enabled && !state.effective_enabled ? t("settings:mobileDiagnostics.unavailable") : undefined,
      rows: state ? [{
        id: "diagnostics-telemetry",
        title: t("settings:mobileDiagnostics.telemetry"),
        toggle: { value: state.configured_enabled, onChange: (enabled) => void change(enabled) },
      }] : [],
    },
    {
      footer: t("settingsAbout:log.capture.description"),
      rows: [{
        id: "diagnostics-capture",
        title: capture.active ? t("settingsAbout:log.capture.stop") : t("settingsAbout:log.capture.start"),
        description: capture.active
          ? t("settingsAbout:log.capture.remaining", { count: capture.remainingMinutes })
          : undefined,
        onPress: () => void capture.toggle(),
      }],
    },
    {
      footer: t("settings:live.sessionLogsFooter"),
      rows: [{
        id: "diagnostics-export",
        title: exporting ? t("settingsIos:diagnostics.action.preparing") : t("settings:mobileDiagnostics.export"),
        description: t("settings:live.sessionLogsSummary"),
        onPress: () => void exportArchive(),
      }],
    },
  ]} />;
}
