import { useNavigation } from "@react-navigation/native";
import type { NativeStackNavigationProp } from "@react-navigation/native-stack";
import type { RootStackParamList } from "@/navigation/AppNavigator.types";
import { useTranslation } from "react-i18next";
import { SettingsPage } from "@/shell/SettingsPage";

import { showUnavailable } from "@/shell/notice";
import type { SpaceDeviceTarget } from "@/navigation/AppNavigator.types";
export function DevicesScreen(_props: SpaceDeviceTarget) {
  const { t } = useTranslation();
  const navigation =
    useNavigation<NativeStackNavigationProp<RootStackParamList>>();
  return (
    <>
      <SettingsPage
        title={t("home:nav.devices")}
        groups={[
          {
            title: t("settings:syncChannel.title"),
            footer: t("shell:unavailable"),
            rows: [
              {
                id: "devices-add",
                title: t("shell:addConnection"),
                onPress: () => navigation.navigate("ConnectionPreview"),
              },
            ],
          },
          {
            title: t("shell:demoDevices"),
            rows: ["Demo Mac", "Demo Phone"].map((name, index) => ({
              id: `device-${index}`,
              title: name,
              value: t("shell:disconnected"),
              onPress: showUnavailable,
            })),
          },
        ]}
      />
    </>
  );
}
