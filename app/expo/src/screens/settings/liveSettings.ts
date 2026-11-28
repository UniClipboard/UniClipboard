import { useCallback, useEffect } from "react";
import { useFocusEffect } from "@react-navigation/native";
import { Alert, Linking } from "react-native";
import { nativeApplicationVersion } from "expo-application";
import { useShallow } from "zustand/react/shallow";
import { useTranslation } from "react-i18next";
import { useEngineStore } from "@/engine/store";
import type { EngineSettings } from "@/engine/types";
import { useTheme } from "@/hooks/useTheme";
import { useAppLanguage } from "@/i18n/useAppLanguage";
import { cloudStatusLabel } from "@/lib/cloudAuth";
import type { SettingsSubSection } from "@/navigation/AppNavigator.types";
import type { ShellGroup, ShellRow } from "@/shell/settings";
import { useCloudStore } from "@/stores/cloudStore";
import { useMessageStore } from "@/stores/messageStore";
import { formatFileSize } from "@/utils";

import { supportsBackgroundSync, useBackgroundSyncGroups } from "./backgroundSync";

type SettingsMutation = (value: EngineSettings) => EngineSettings;
export type SettingEdit = {
  id: string;
  title: string;
  value: string;
  numeric?: boolean;
  save: (value: string) => Promise<void>;
};

export function useLiveSettingGroups(
  page: SettingsSubSection | "root",
  edit: (value: SettingEdit) => void,
  open: (section: SettingsSubSection) => void,
): ShellGroup[] {
  const { t } = useTranslation();
  const state = useEngineStore(useShallow((state) => ({
    snapshot: state.snapshot, capabilities: state.capabilities, devices: state.devices,
    lastError: state.lastError, fileCache: state.fileCache, autoApplyReceived: state.autoApplyReceived,
    automaticCapture: state.automaticCapture, updateSettings: state.updateSettings,
    setAutoApplyReceived: state.setAutoApplyReceived, setAutomaticCapture: state.setAutomaticCapture,
    beginLaterMigration: state.beginLaterMigration, clearPreviews: state.clearPreviews,
  })));
  const { themeMode, setThemeMode } = useTheme();
  const { preference, setLanguage } = useAppLanguage();
  const cloud = useCloudStore(useShallow((s) => ({
    status: s.status,
    busy: s.busy,
    hydrate: s.hydrate,
    signIn: s.signIn,
    signOut: s.signOut,
  })));
  useEffect(() => {
    void cloud.hydrate();
  }, [cloud.hydrate]);
  const settings = state.snapshot?.settings.value;
  const background = useBackgroundSyncGroups(page === "background");

  useFocusEffect(useCallback(() => {
    const engine = useEngineStore.getState();
    if (page === "storage") {
      void engine.refreshFileCache();
      const timer = setInterval(() => void engine.refreshFileCache(), 3000);
      return () => clearInterval(timer);
    }
    if (page === "clipboard" || page === "network") void engine.refreshSnapshot();
  }, [page]));

  const run = (promise: Promise<unknown>) => {
    void promise.catch((error) => Alert.alert(
      t("settingsStorage:setFailed"),
      error instanceof Error ? error.message : undefined,
    ));
  };
  const switchRow = (id: string, title: string, value: boolean, change: (value: boolean) => Promise<void>): ShellRow =>
    ({ id, title, toggle: { value, onChange: (next) => run(change(next)) } });
  const field = (
    id: string, title: string, value: string | number, mutate: (value: string) => SettingsMutation,
    displayValue?: string,
  ): ShellRow => ({
    id, title, value: displayValue ?? String(value),
    onPress: () => edit({
      id, title, value: String(value), numeric: typeof value === "number",
      save: (next) => state.updateSettings(mutate(next.trim())),
    }),
  });

  const integer = (value: string) => {
    const parsed = Number(value);
    if (!/^\d+$/.test(value) || !Number.isSafeInteger(parsed)) {
      throw new Error(t("settings:live.integer"));
    }
    return parsed;
  };
  const nav = (id: SettingsSubSection, title = t(`settings:category.${id}`)): ShellRow =>
    ({ id, title, onPress: () => open(id) });

  if (page === "root") return [
    {
      title: t("settings:live.cloudSection"),
      footer: t("settings:live.cloudFooter"),
      rows: cloud.status?.signed_in
        ? [
            {
              id: "cloud-account",
              title: cloudStatusLabel(cloud.status),
              value: t("settings:live.cloudSignedIn"),
            },
            {
              id: "cloud-sign-out",
              title: t("settings:live.cloudSignOut"),
              onPress: () => run(cloud.signOut()),
            },
          ]
        : cloud.status?.reauth_required
          ? [
              {
                id: "cloud-account",
                title: cloudStatusLabel(cloud.status),
                value: t("settings:live.cloudReauthRequired"),
              },
              {
                id: "cloud-sign-in",
                title: cloud.busy ? t("settings:live.cloudSigningIn") : t("settings:live.cloudReauthAction"),
                description: t("settings:live.cloudReauthRequiredDesc"),
                onPress: () => {
                  if (cloud.busy) return;
                  void cloud
                    .signIn()
                    .then(() => Alert.alert(
                      t("settings:live.cloudSignInSuccessTitle"),
                      t("settings:live.cloudSignInSuccessBody"),
                    ))
                    .catch((error) => Alert.alert(
                      t("settings:live.cloudSignInFailed"),
                      error instanceof Error ? error.message : undefined,
                    ));
                },
              },
              {
                id: "cloud-sign-out",
                title: t("settings:live.cloudSignOut"),
                onPress: () => run(cloud.signOut()),
              },
            ]
          : [
              {
                id: "cloud-sign-in",
                title: cloud.busy ? t("settings:live.cloudSigningIn") : t("settings:live.cloudSignIn"),
                description: t("settings:live.cloudSignInDesc"),
                onPress: () => {
                  if (cloud.busy) return;
                  void cloud
                    .signIn()
                    .then(() => Alert.alert(
                      t("settings:live.cloudSignInSuccessTitle"),
                      t("settings:live.cloudSignInSuccessBody"),
                    ))
                    .catch((error) => Alert.alert(
                      t("settings:live.cloudSignInFailed"),
                      error instanceof Error ? error.message : undefined,
                    ));
                },
              },
            ],
    },
    { rows: [
      nav("sync"),
      nav("history"),
      nav("storage"),
      nav("network"),
      ...(supportsBackgroundSync ? [nav("background")] : []),
      nav("clipboard", t("settings:ios.extensions.clipboardAccess")),
      nav("diagnostics"),
    ] },
    { title: t("settings:general.sectionTitle"), rows: [
      {
        id: "appearance", title: t("settings:general.theme"),
        choices: {
          value: themeMode,
          options: ["auto", "light", "dark"].map((value) => ({
            value, label: t(`settings:appearance.mode.${value === "auto" ? "system" : value}`),
          })),
          onChange: (value) => void setThemeMode(value as typeof themeMode),
        },
      },
      {
        id: "language", title: t("common:language.title"),
        choices: {
          value: preference,
          options: [
            { value: "system", label: t("common:language.system") },
            { value: "zh-CN", label: "简体中文" },
            { value: "en", label: "English" },
            { value: "ru", label: "Русский" },
            { value: "pt-BR", label: "Português" },
          ],
          onChange: (value) => void setLanguage(value as typeof preference),
        },
      },
      nav("about"),
    ] },
  ];
  if (page === "background") return background;
  if (!settings) {
    const message = state.lastError?.message ?? t("settings:live.loading");
    return [{ rows: [{ id: "engine-status", title: message }] }];
  }
  if (page === "sync") {
    const switches = [
      ["enabled", "syncEnabled"], ["automatic", "syncAutomatic"], ["on_restore", "syncRestore"],
      ["files", "syncFiles"], ["introductions", "syncIntroductions"],
    ] as const;
    return [
      { rows: [
        field("device-name", t("settings:live.deviceName"), settings.device_name,
          (device_name) => (old) => ({ ...old, device_name }), settings.device_name || state.snapshot?.self.name),
        ...switches.map(([key, label]) => switchRow(
          `sync-${key}`, t(`settings:live.${label}`), settings.sync[key],
          (value) => state.updateSettings((old) => ({ ...old, sync: { ...old.sync, [key]: value } })),
        )),
      ] },
      { footer: t("settings:live.captureHint"), rows: [
        switchRow("auto-apply", t("settings:hub.clipboardSync.autoApply.title"),
          state.autoApplyReceived, state.setAutoApplyReceived),
        switchRow("auto-capture", t("settings:live.capture"), state.automaticCapture, state.setAutomaticCapture),
      ] },
    ];
  }
  if (page === "history") {
    const tags = settings.history.excluded_tags ?? [];
    return [
      { rows: [
        switchRow("history-cleanup", t("settings:live.cleanup"), settings.history.auto_cleanup,
          (auto_cleanup) => state.updateSettings((old) => ({ ...old, history: { ...old.history, auto_cleanup } }))),
        field("retention-days", t("settings:live.retention"), settings.history.retention_days,
          (value) => (old) => ({ ...old, history: { ...old.history, retention_days: integer(value) } })),
        field("history-max-items", t("settingsStorage:history.maxItemsLabel"), settings.history.max_items,
          (value) => (old) => ({ ...old, history: { ...old.history, max_items: integer(value) } })),
      ] },
      {
        title: t("settings:live.excludedTags"),
        footer: t("settings:live.excludedHint"),
        rows: [
          ...["starred", "pinned"].map((tag) => switchRow(
            `exclude-${tag}`, t(`settings:live.${tag}`), tags.includes(tag),
            (enabled) => state.updateSettings((old) => {
              const excluded = new Set(old.history.excluded_tags ?? []);
              if (enabled) excluded.add(tag);
              else excluded.delete(tag);
              return { ...old, history: { ...old.history, excluded_tags: [...excluded] } };
            }),
          )),
          field("excluded-tags", t("settings:live.excludedTags"), tags.join(", "),
            (value) => (old) => ({
              ...old, history: { ...old.history, excluded_tags: value.split(",").map((tag) => tag.trim()).filter(Boolean) },
            })),
        ],
      },
    ];
  }
  if (page === "storage") return [
    { rows: [
      ...(["max_file_bytes", "max_total_bytes"] as const).map((key) => field(
        key, t(`settings:live.${key}`), settings.limits[key],
        (value) => (old) => ({ ...old, limits: { ...old.limits, [key]: integer(value) } }),
        formatFileSize(settings.limits[key]),
      )),
      field("file-cache-limit", t("settings:live.cacheLimit"), settings.file_cache.max_bytes,
        (value) => (old) => ({ ...old, file_cache: { max_bytes: integer(value) } }),
        formatFileSize(settings.file_cache.max_bytes)),
      {
        id: "file-cache-usage", title: t("settingsStorage:cache.usageLabel"),
        value: state.fileCache
          ? formatFileSize(state.fileCache.stored_bytes + state.fileCache.reserved_bytes)
          : "—",
      },
      {
        id: "clear-previews", title: t("settings:live.clearPreviews"),
        onPress: () => run(state.clearPreviews().then(() =>
          useMessageStore.getState().showMessage(t("settingsStorage:cache.cleared"), "success"))),
      },
    ] },
    { rows: [{
      id: "storage-migrate-v1", title: t("settingsStorage:migration.title"),
      description: t("settingsStorage:migration.description"), onPress: () => run(state.beginLaterMigration()),
    }] },
  ];
  if (page === "network") {
    const bindings = state.devices.flatMap((device) => device.bindings ?? [])
      .filter((binding) => binding.instance_id === "builtin.tailcat");
    const connected = bindings.filter((binding) => binding.connection === "connected").length;
    return [{ rows: [
      field("rendezvous-url", t("settings:live.rendezvous"), settings.rendezvous_url,
        (rendezvous_url) => (old) => ({ ...old, rendezvous_url }),
        settings.rendezvous_url || t("settings:live.default")),
      { id: "rendezvous-reset", title: t("settings:live.resetRendezvous"),
        onPress: () => run(state.updateSettings((old) => ({ ...old, rendezvous_url: "" }))) },
      { id: "network-runtime", title: t("settings:live.runtime"),
        value: state.snapshot?.runtime ? t(`settings:live.status.${state.snapshot.runtime}`) : "—" },
      { id: "network-tailcat", title: t("settings:live.tailcat"),
        value: t("settings:live.connected", { connected, total: bindings.length }) },
    ] }];
  }
  if (page === "clipboard") {
    const capabilities = state.capabilities;
    return [{ rows: [
      { id: "clipboard-observe", title: t("settings:live.observe"),
        value: capabilities ? t(`settings:live.status.${capabilities.effective.observe}`) : "—" },
      { id: "clipboard-apply", title: t("settings:live.apply"),
        value: capabilities ? t(`settings:live.status.${capabilities.effective.apply}`) : "—" },
      ...(capabilities?.restrictions ?? []).map((reason) => ({
        id: `restriction-${reason}`, title: t(`settings:live.status.${reason}`, { defaultValue: reason }),
      })),
      { id: "clipboard-system-settings", title: t("settings:openSystemSettings"), onPress: () => run(Linking.openSettings()) },
    ] }];
  }
  if (page === "about") return [{ rows: [
    { id: "about-name", title: "Unic" },
    { id: "about-version", title: t("settings:ios.about.version"), value: nativeApplicationVersion ?? "—" },
  ] }];
  return [];
}
