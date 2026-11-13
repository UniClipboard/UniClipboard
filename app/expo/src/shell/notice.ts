import { Alert } from "react-native";
import i18n from "@/i18n";

export function showUnavailable(): void {
  Alert.alert(i18n.t("shell:title"), i18n.t("shell:unavailable"));
}
