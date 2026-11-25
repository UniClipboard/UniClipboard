import { useCallback, useEffect, useRef, useState } from "react";
import { Alert, AppState, ScrollView, Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { AppBottomSheet, AppButton, AppHost } from "@/components/ui";
import { parseEngineError, unicEngineNative } from "@/engine/nativeModule";
import { useEngineStore } from "@/engine/store";
import { useTheme } from "@/hooks/useTheme";

export function IncomingShareSheet() {
  const { t } = useTranslation();
  const { theme } = useTheme();
  const devices = useEngineStore((s) => s.devices);
  const [items, setItems] = useState<Awaited<ReturnType<typeof unicEngineNative.listIncomingShares>>>([]);
  const [target, setTarget] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const pending = useRef(false);
  const request = useRef(0);
  const item = items[0];

  const reload = useCallback(async () => {
    const seq = ++request.current;
    try {
      const current = await unicEngineNative.listIncomingShares();
      if (seq === request.current) setItems(current);
    } catch {
      // A later foreground transition or native event retries the read.
    }
  }, []);

  useEffect(() => {
    const shares = unicEngineNative.addIncomingShareListener(() => void reload());
    const app = AppState.addEventListener("change", (state) => {
      if (state === "active") void reload();
    });
    void reload();
    return () => {
      request.current++;
      shares.remove();
      app.remove();
    };
  }, [reload]);

  useEffect(() => setTarget(null), [item?.id]);

  const handle = async (action: "save" | "send" | "dismiss") => {
    if (!item || pending.current || (action === "send" && !target)) return;
    pending.current = true;
    setBusy(true);
    try {
      if (action === "dismiss") {
        await unicEngineNative.dismissIncomingShare(item.id);
      } else {
        const submission = await unicEngineNative.commitIncomingShare(item.id, action === "send" && target ? [target] : undefined);
        if (action === "send" && submission.sync_disposition !== "queued") {
          Alert.alert(t(submission.history_id ? "share:incoming.savedWithoutSend" : "share:incoming.sendNotStarted"));
        } else if (action === "save" && !submission.history_id) {
          Alert.alert(t("share:incoming.notSaved"));
        }
        void useEngineStore.getState().refreshHistory();
      }
      await reload();
    } catch (error) {
      Alert.alert(t("share:incoming.failed"), t(`errors:engine.${parseEngineError(error).code}`, {
        defaultValue: t("errors:engine.internal"),
      }));
    } finally {
      pending.current = false;
      setBusy(false);
    }
  };

  return (
    <AppBottomSheet visible={Boolean(item)} onDismiss={() => void handle("dismiss")}>
      {item && (
        <View style={{ padding: 16, gap: 12 }}>
          <Text accessibilityRole="header" style={{ color: theme.colors.textPrimary, fontSize: 18, fontWeight: "700" }}>
            {t("share:incoming.title")}
          </Text>
          <Text numberOfLines={4} style={{ color: theme.colors.textPrimary }}>{item.label}</Text>
          <AppHost matchContents={{ vertical: true }}>
            <AppButton testID="incoming-share-save" title={t("share:incoming.save")} disabled={busy}
              variant="filled" fullWidth onPress={() => void handle("save")} />
          </AppHost>
          <ScrollView style={{ maxHeight: 180 }}>
            {devices.filter((device) => device.trust === "trusted").map((device) => (
              <AppHost key={device.id} matchContents={{ vertical: true }}>
                <AppButton title={device.name || device.reported_name || device.id}
                  variant={target === device.id ? "filled" : "tonal"} fullWidth disabled={busy}
                  onPress={() => setTarget(target === device.id ? null : device.id)} />
              </AppHost>
            ))}
          </ScrollView>
          {target && (
            <AppHost matchContents={{ vertical: true }}>
              <AppButton testID="incoming-share-send" title={t("share:incoming.send")} variant="outlined"
                disabled={busy} fullWidth onPress={() => void handle("send")} />
            </AppHost>
          )}
          <AppHost matchContents={{ vertical: true }}>
            <AppButton title={t("common:action.cancel")} disabled={busy} variant="text" fullWidth
              onPress={() => void handle("dismiss")} />
          </AppHost>
        </View>
      )}
    </AppBottomSheet>
  );
}
