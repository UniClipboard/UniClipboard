import { useCallback, useEffect, useState } from "react";
import { Alert, AppState, Linking } from "react-native";
import { applicationId } from "expo-application";
import { parseEngineError, unicEngineNative } from "@/engine/nativeModule";
import { useEngineStore } from "@/engine/store";
import i18n from "@/i18n";
import { SendSheet } from "./SendSheet";
import { useSendSheetStore } from "./sendSheetStore";

const shareURL = applicationId ? `${applicationId.toLowerCase()}://share` : null;

export function SendSheetHost() {
  const mode = useEngineStore((state) => state.mode);
  const request = useSendSheetStore((state) => state.queue[0]);
  const finish = useSendSheetStore((state) => state.finish);
  const [closing, setClosing] = useState(false);
  const close = useCallback(() => {
    if (request && useSendSheetStore.getState().queue[0] === request) setClosing(true);
  }, [request]);
  const didDismiss = useCallback(() => {
    finish();
    setClosing(false);
  }, [finish]);

  useEffect(() => {
    if (mode !== "live") return;
    let disposed = false;
    let running = false;
    let requested = false;
    const reportFailure = (error: unknown) => {
      Alert.alert(i18n.t("share:incoming.failed"), i18n.t(`errors:engine.${parseEngineError(error).code}`, {
        defaultValue: i18n.t("errors:engine.internal"),
      }));
    };

    const importShares = async () => {
      requested = true;
      if (running) return;
      running = true;
      try {
        while (requested && !disposed) {
          requested = false;
          for (const id of await unicEngineNative.listIncomingShares()) {
            if (disposed) return;
            try {
              const item = await useEngineStore.getState().saveIncomingShare(id);
              if (disposed) return;
              if (item) useSendSheetStore.getState().open({ item });
              else Alert.alert(i18n.t("share:incoming.notSaved"));
            } catch (error) {
              if (!disposed) reportFailure(error);
            }
          }
        }
      } catch (error) {
        if (!disposed) reportFailure(error);
      } finally {
        running = false;
      }
    };
    const incoming = unicEngineNative.addIncomingShareListener(() => void importShares());
    const link = Linking.addEventListener("url", ({ url }) => {
      if (url.toLowerCase() === shareURL) void importShares();
    });
    const app = AppState.addEventListener("change", (state) => {
      if (state === "active") void importShares();
    });
    void importShares();
    return () => {
      disposed = true;
      incoming.remove();
      link.remove();
      app.remove();
    };
  }, [mode]);

  return (
    <SendSheet
      visible={Boolean(request) && !closing}
      targetItem={request?.item}
      initialDraftText={request?.draftText}
      onClose={close}
      onDidDismiss={didDismiss}
    />
  );
}
