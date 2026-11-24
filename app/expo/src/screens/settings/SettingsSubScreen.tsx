import { useNavigation, useRoute, type RouteProp } from "@react-navigation/native";
import type { RootStackParamList } from "@/navigation/AppNavigator.types";
import { SettingsScreenContent } from "./SettingsScreenContent";

export function SettingsSubScreen() {
  const navigation = useNavigation();
  const { params } = useRoute<RouteProp<RootStackParamList, "SettingsSub">>();
  return <SettingsScreenContent page={params.section} onBack={() => navigation.goBack()} />;
}
