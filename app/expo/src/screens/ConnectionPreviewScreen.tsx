import { showUnavailable } from "@/shell/notice";
import { useNavigation } from "@react-navigation/native";
import type { NativeStackNavigationProp } from "@react-navigation/native-stack";
import { useTranslation } from "react-i18next";
import type { RootStackParamList } from "@/navigation/AppNavigator.types";
import { OnboardingConnection } from "./onboarding/OnboardingConnection";

export function ConnectionPreviewScreen() {
  const navigation =
    useNavigation<
      NativeStackNavigationProp<RootStackParamList, "ConnectionPreview">
    >();
  const { t } = useTranslation("settingsAbout");
  const stage = "confirm" as const;
  const close = () => navigation.goBack();

  return (
    <OnboardingConnection
      connection={{
        stage,
        intent: {
          name: t("debug.connectionPreviewComputer"),
          urls: [
            "http://computer.example.invalid:42720",
            "https://backup.example.invalid",
          ],
          username: "preview",
          password: "",
        },
        busy: false,
        error: null,
        scan: async () => showUnavailable(),
        connect: async () => showUnavailable(),
        close,
      }}
      onComplete={close}
      finishing={false}
      completionError={false}
    />
  );
}
