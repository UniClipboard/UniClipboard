import {
  NavigationContainer,
  DefaultTheme,
  DarkTheme,
} from "@react-navigation/native";
import { createNativeStackNavigator } from "@react-navigation/native-stack";
import { Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { useTheme } from "@/hooks/useTheme";
import { OnboardingPreviewScreen } from "@/screens/OnboardingPreviewScreen";
import { SettingsScreen } from "@/screens/SettingsScreen";
import { SettingsSubScreen } from "@/screens/settings/SettingsSubScreen";
import { ConnectionPreviewScreen } from "@/screens/ConnectionPreviewScreen";
import { MainScreen } from "./MainScreen";
import {
  SafeAreaInsetsContext,
  useSafeAreaInsets,
} from "react-native-safe-area-context";
import type { RootStackParamList } from "./AppNavigator.types";
export type {
  RootStackParamList,
  SettingsSubSection,
} from "./AppNavigator.types";
const Stack = createNativeStackNavigator<RootStackParamList>();

export function AppNavigator() {
  const { theme } = useTheme();
  const { t } = useTranslation();
  const insets = useSafeAreaInsets();
  const base = theme.isDark ? DarkTheme : DefaultTheme;
  return (
    <View
      style={{
        flex: 1,
        paddingTop: insets.top,
        backgroundColor: theme.colors.background,
      }}
    >
      <View
        style={{ padding: 8, backgroundColor: theme.colors.accentContainer }}
      >
        <Text
          testID="shell-banner"
          style={{ color: theme.colors.onAccentContainer, textAlign: "center" }}
        >
          {t("shell:banner")}
        </Text>
      </View>
      <SafeAreaInsetsContext.Provider value={{ ...insets, top: 0 }}>
        <NavigationContainer
          theme={{
            ...base,
            colors: {
              ...base.colors,
              background: theme.colors.background as string,
              card: theme.colors.surface as string,
              text: theme.colors.textPrimary as string,
              primary: theme.colors.accent as string,
            },
          }}
        >
          <Stack.Navigator
            initialRouteName="Main"
            screenOptions={{ headerShown: false }}
          >
            <Stack.Screen name="Main" component={MainScreen} />
            <Stack.Screen
              name="OnboardingPreview"
              component={OnboardingPreviewScreen}
              options={{ presentation: "fullScreenModal" }}
            />
            <Stack.Screen
              name="ConnectionPreview"
              component={ConnectionPreviewScreen}
              options={{ presentation: "fullScreenModal" }}
            />
            <Stack.Screen name="Settings" component={SettingsScreen} />
            <Stack.Screen name="SettingsSub" component={SettingsSubScreen} />
          </Stack.Navigator>
        </NavigationContainer>
      </SafeAreaInsetsContext.Provider>
    </View>
  );
}
