import { useTranslation } from "react-i18next";
import { useNavigation } from "@react-navigation/native";
import type { NativeStackNavigationProp } from "@react-navigation/native-stack";
import { useTheme } from "@/hooks/useTheme";
import { useAppLanguage } from "@/i18n/useAppLanguage";
import { useShellStore } from "./store";
import { showUnavailable } from "./notice";
import type { RootStackParamList } from "@/navigation/AppNavigator.types";

export type ShellSettingPage =
  | "history"
  | "storage"
  | "keyboard"
  | "share"
  | "clipboard"
  | "background"
  | "diagnostics"
  | "privacy"
  | "about"
  | "developer";
export type ShellRow = {
  id: string;
  title: string;
  description?: string;
  value?: string;
  onPress?: () => void;
  toggle?: { value: boolean; onChange: (value: boolean) => void };
  choices?: {
    value: string;
    options: { value: string; label: string }[];
    onChange: (value: string) => void;
  };
};
export type ShellGroup = { title?: string; footer?: string; rows: ShellRow[] };

export function useShellSettingGroups(
  page: ShellSettingPage | "root",
): ShellGroup[] {
  const { t } = useTranslation();
  const state = useShellStore();
  const { themeMode, setThemeMode } = useTheme();
  const { preference, setLanguage } = useAppLanguage();
  const navigation =
    useNavigation<NativeStackNavigationProp<RootStackParamList>>();
  const open = (section: ShellSettingPage) =>
    navigation.navigate("SettingsSub", { section });
  if (page === "root")
    return [
      {
        title: t("settings:hub.clipboardSync.title"),
        footer: t("shell:localOnly"),
        rows: [
          {
            id: "auto-apply",
            title: t("settings:hub.clipboardSync.autoApply.title"),
            toggle: {
              value: state.autoApplyRemote,
              onChange: (autoApplyRemote) =>
                state.setPreference({ autoApplyRemote }),
            },
          },
          {
            id: "auto-push",
            title: t("settings:hub.clipboardSync.autoPush.title"),
            toggle: {
              value: state.autoPushLocal,
              onChange: (autoPushLocal) =>
                state.setPreference({ autoPushLocal }),
            },
          },
        ],
      },
      {
        title: t("settings:general.sectionTitle"),
        rows: [
          {
            id: "history",
            title: t("settings:category.history"),
            value: String(state.maxHistoryItems),
            onPress: () => open("history"),
          },
          {
            id: "appearance",
            title: t("settings:general.theme"),
            choices: {
              value: themeMode,
              options: ["auto", "light", "dark"].map((value) => ({
                value,
                label: t(
                  `settings:appearance.mode.${value === "auto" ? "system" : value}`,
                ),
              })),
              onChange: (value) => void setThemeMode(value as typeof themeMode),
            },
          },
          {
            id: "language",
            title: t("common:language.title"),
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
          ...(["clipboard", "storage", "background"] as const).map(
            (section) => ({
              id: section,
              title: t(
                section === "clipboard"
                  ? "settings:ios.extensions.clipboardAccess"
                  : `settings:category.${section}`,
              ),
              onPress: () => open(section),
            }),
          ),
        ],
      },
      {
        title: t("settings:category.extensions"),
        rows: (["keyboard", "share"] as const).map((section) => ({
          id: section,
          title: t(`settings:ios.extensions.${section}`),
          onPress: () => open(section),
        })),
      },
      {
        title: t("settings:category.other"),
        rows: (["diagnostics", "privacy", "about", "developer"] as const).map(
          (section) => ({
            id: section,
            title: t(`settings:category.${section}`),
            onPress: () => open(section),
          }),
        ),
      },
    ];
  if (page === "history")
    return [
      {
        footer: t("shell:localOnly"),
        rows: [
          {
            id: "history-max-items",
            title: t("settingsStorage:history.maxItemsLabel"),
            choices: {
              value: String(state.maxHistoryItems),
              options: [100, 200, 500, 1000].map((value) => ({
                value: String(value),
                label: String(value),
              })),
              onChange: (value) =>
                state.setPreference({ maxHistoryItems: Number(value) }),
            },
          },
          {
            id: "history-reset",
            title: t("shell:reset"),
            onPress: state.resetHistory,
          },
        ],
      },
    ];
  if (page === "storage")
    return [
      {
        rows: [
          {
            id: "storage-count",
            title: t("settings:category.history"),
            value: String(state.records.length),
          },
          {
            id: "storage-reset",
            title: t("shell:reset"),
            onPress: state.resetHistory,
          },
        ],
        footer: t("shell:localOnly"),
      },
    ];
  if (page === "about")
    return [
      {
        rows: [
          { id: "about-name", title: "UniClipboard", value: "Expo UI Shell" },
          {
            id: "about-source",
            title: t("shell:source"),
            value: "UniClip · ac90f94",
          },
          {
            id: "about-onboarding",
            title: t("shell:onboarding"),
            onPress: () => navigation.navigate("OnboardingPreview"),
          },
        ],
      },
    ];
  if (page === "developer")
    return [
      {
        rows: [
          {
            id: "developer-reset",
            title: t("shell:reset"),
            onPress: state.resetHistory,
          },
          {
            id: "developer-onboarding",
            title: t("shell:onboarding"),
            onPress: () => navigation.navigate("OnboardingPreview"),
          },
        ],
      },
    ];
  return [
    {
      footer: t("shell:unavailable"),
      rows: [
        {
          id: `${page}-unavailable`,
          title: t("shell:integration"),
          value: t("shell:disconnected"),
          onPress: showUnavailable,
        },
      ],
    },
  ];
}
