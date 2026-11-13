import { useTranslation } from "react-i18next";
import { SettingsPage } from "@/shell/SettingsPage";
import { useShellSettingGroups } from "@/shell/settings";
export function SettingsScreen() {
  const { t } = useTranslation("settings");
  return (
    <SettingsPage
      title={t("action.settings", { ns: "common" })}
      groups={useShellSettingGroups("root")}
    />
  );
}
