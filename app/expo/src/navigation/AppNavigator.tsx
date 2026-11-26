import { useEffect, useRef } from "react";
import {
  NavigationContainer,
  DefaultTheme,
  DarkTheme,
} from "@react-navigation/native";
import { createNativeStackNavigator } from "@react-navigation/native-stack";
import { ActivityIndicator, Platform, Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { M3IconButton } from "@/components/android/M3IconButton";
import { useEngineStore } from "@/engine/store";
import { useTheme } from "@/hooks/useTheme";
import { MigrationSetupScreen } from "@/screens/setup/MigrationSetupScreen";
import { OnboardingScreen } from "@/screens/OnboardingScreen";
import { OnboardingPreviewScreen } from "@/screens/OnboardingPreviewScreen";
import { SettingsScreen } from "@/screens/SettingsScreen";
import { SettingsSubScreen } from "@/screens/settings/SettingsSubScreen";
import { ConnectionPreviewScreen } from "@/screens/ConnectionPreviewScreen";
import { MainScreen } from "./MainScreen";
import {
  SafeAreaInsetsContext,
  useSafeAreaInsets,
} from "react-native-safe-area-context";
import { SendSheetHost } from "@/screens/SendSheetHost";
import { hideLaunchSplash } from "@/utils/splashScreen";
import type { RootStackParamList, SettingsSubSection } from "./AppNavigator.types";
export type {
  RootStackParamList,
  SettingsSubSection,
} from "./AppNavigator.types";
const Stack = createNativeStackNavigator<RootStackParamList>();

export function AppNavigator() {
  const { theme, isThemeReady } = useTheme();
  const { t } = useTranslation();
  const insets = useSafeAreaInsets();
  const base = theme.isDark ? DarkTheme : DefaultTheme;
  const mode = useEngineStore((s) => s.mode);
  const bootStage = useEngineStore((s) => s.bootStage);
  const bootstrap = useEngineStore((s) => s.bootstrap);
  const completeSetupAndEnterMain = useEngineStore((s) => s.completeSetupAndEnterMain);
  const splashHiddenRef = useRef(false);

  useEffect(() => {
    void bootstrap();
  }, [bootstrap]);

  useEffect(() => {
    if (splashHiddenRef.current || bootStage === "loading" || !isThemeReady) {
      return;
    }
    let frame1 = 0;
    let frame2 = 0;
    frame1 = requestAnimationFrame(() => {
      frame2 = requestAnimationFrame(() => {
        if (!splashHiddenRef.current) {
          splashHiddenRef.current = true;
          hideLaunchSplash();
        }
      });
    });
    return () => {
      cancelAnimationFrame(frame1);
      cancelAnimationFrame(frame2);
    };
  }, [bootStage, isThemeReady]);

  if (bootStage === "loading") {
    return (
      <View
        style={{
          flex: 1,
          alignItems: "center",
          justifyContent: "center",
          backgroundColor: theme.colors.background,
        }}
      >
        <ActivityIndicator size="large" color={theme.colors.accent} />
      </View>
    );
  }

  if (mode === "live" && bootStage === "setup") {
    return <MigrationSetupScreen />;
  }

  if (mode === "live" && bootStage === "onboarding") {
    return <OnboardingScreen onComplete={completeSetupAndEnterMain} />;
  }

  const subScreenTitle = (section: SettingsSubSection) =>
    section === "clipboard"
      ? t("settings:ios.extensions.clipboardAccess")
      : section === "keyboard" || section === "share"
        ? t(`settings:ios.extensions.${section}`)
        : t(`settings:category.${section}`, { defaultValue: section });

  const isAndroid = Platform.OS === "android";

  const navigator = (
    <>
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
          <Stack.Screen
            name="Settings"
            component={SettingsScreen}
            options={
              isAndroid
                ? {
                    headerShown: true,
                    title: t("common:action.settings"),
                    presentation: "card",
                    animation: "slide_from_right",
                    headerStyle: { backgroundColor: theme.colors.background as string },
                    headerShadowVisible: false,
                    headerTintColor: theme.colors.textPrimary as string,
                  }
                : undefined
            }
          />
          <Stack.Screen
            name="SettingsSub"
            component={SettingsSubScreen}
            options={
              isAndroid
                ? ({ navigation, route }) => ({
                    headerShown: true,
                    title: subScreenTitle(route.params.section),
                    presentation: "card",
                    animation: "slide_from_right",
                    headerStyle: { backgroundColor: theme.colors.background as string },
                    headerShadowVisible: false,
                    headerTintColor: theme.colors.textPrimary as string,
                    headerLeft: () => (
                      <M3IconButton
                        testID="settings-back"
                        icon="arrow-back"
                        accessibilityLabel={t("common:action.back")}
                        onPress={() => navigation.goBack()}
                        colors={theme.colors}
                        iconColor={theme.colors.textPrimary}
                      />
                    ),
                  })
                : undefined
            }
          />
        </Stack.Navigator>
      </NavigationContainer>
      <SendSheetHost />
    </>
  );

  if (mode === "live") {
    return navigator;
  }

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
        {navigator}
      </SafeAreaInsetsContext.Provider>
    </View>
  );
}
