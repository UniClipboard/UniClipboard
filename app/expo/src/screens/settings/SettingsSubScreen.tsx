import {
  useNavigation,
  useRoute,
  type RouteProp,
} from "@react-navigation/native";
import { useTranslation } from "react-i18next";
import { SettingsPage } from "@/shell/SettingsPage";
import { useShellSettingGroups, type ShellSettingPage } from "@/shell/settings";
import type { RootStackParamList } from "@/navigation/AppNavigator.types";
export function SettingsSubScreen() {
  const navigation = useNavigation();
  const { params } = useRoute<RouteProp<RootStackParamList, "SettingsSub">>();
  const { t } = useTranslation();
  return (
    <SettingsPage
      title={t(`settings:category.${params.section}`, {
        defaultValue: params.section,
      })}
      groups={useShellSettingGroups(params.section as ShellSettingPage)}
      onBack={() => navigation.goBack()}
    />
  );
}
