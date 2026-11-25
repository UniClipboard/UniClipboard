import { useEffect, useRef, useState } from "react";
import { Alert } from "react-native";
import { useTranslation } from "react-i18next";
import { type TelemetryState, unicEngineNative } from "@/engine/nativeModule";
import { SettingsPage } from "./SettingsPage";

export function DiagnosticsSettings({ title, onBack }: { title: string; onBack?: () => void }) {
  const { t } = useTranslation("settings");
  const [state, setState] = useState<TelemetryState>();
  const changing = useRef(false);

  useEffect(() => {
    let live = true;
    void unicEngineNative.getTelemetryState().then((value) => {
      if (live) setState(value);
    }, () => {
      if (live) Alert.alert(t("mobileDiagnostics.failed"));
    });
    return () => { live = false; };
  }, [t]);

  const change = async (enabled: boolean) => {
    if (changing.current) return;
    changing.current = true;
    try {
      setState(await unicEngineNative.setTelemetryEnabled(enabled));
    } catch {
      Alert.alert(t("mobileDiagnostics.failed"));
    } finally {
      changing.current = false;
    }
  };

  const exportLogs = async () => {
    try {
      const logs = await unicEngineNative.exportRecentDiagnosticLogs();
      if (logs) await unicEngineNative.shareHistoryMedia([], logs);
      else Alert.alert(t("mobileDiagnostics.empty"));
    } catch {
      Alert.alert(t("mobileDiagnostics.failed"));
    }
  };

  return <SettingsPage title={title} onBack={onBack} groups={[
    {
      footer: state?.configured_enabled && !state.effective_enabled ? t("mobileDiagnostics.unavailable") : undefined,
      rows: state ? [{
        id: "diagnostics-telemetry",
        title: t("mobileDiagnostics.telemetry"),
        toggle: { value: state.configured_enabled, onChange: (enabled) => void change(enabled) },
      }] : [],
    },
    { rows: [{ id: "diagnostics-export", title: t("mobileDiagnostics.export"), onPress: () => void exportLogs() }] },
  ]} />;
}
