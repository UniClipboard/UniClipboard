import { useCallback, useRef, useState } from "react";
import { Alert } from "react-native";
import { useFocusEffect } from "@react-navigation/native";
import { useTranslation } from "react-i18next";
import { parseEngineError, unicEngineNative } from "@/engine/nativeModule";
import type { ShellGroup } from "@/shell/settings";

export const supportsBackgroundSync = true;

export function useBackgroundSyncGroups(visible: boolean): ShellGroup[] {
  const { t } = useTranslation();
  const [value, setValue] = useState<boolean | null>(null);
  const busy = useRef(false);

  useFocusEffect(useCallback(() => {
    if (!visible) return;
    let current = true;
    void unicEngineNative.getBackgroundSyncEnabled().then((enabled) => {
      if (current) setValue(enabled);
    }).catch(() => {
      if (current) setValue(null);
    });
    return () => { current = false; };
  }, [visible]));

  return [{
    footer: t("settingsBackground:live.description"),
    rows: [value === null ? {
      id: "background-sync-loading", title: t("settings:live.loading"),
    } : {
      id: "background-sync",
      title: t("settingsBackground:main.cardTitle"),
      toggle: {
        value,
        onChange: (enabled) => {
          if (busy.current) return;
          busy.current = true;
          void unicEngineNative.setBackgroundSyncEnabled(enabled).then(setValue).catch((error) => {
            Alert.alert(t("settingsStorage:setFailed"), t(`errors:engine.${parseEngineError(error).code}`, {
              defaultValue: t("errors:engine.internal"),
            }));
          }).finally(() => { busy.current = false; });
        },
      },
    }],
  }];
}
