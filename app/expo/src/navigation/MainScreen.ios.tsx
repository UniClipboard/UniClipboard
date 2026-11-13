import { useCallback, useMemo, useState } from "react";
import {
  useNavigation,
  type CompositeNavigationProp,
} from "@react-navigation/native";
import {
  createNativeBottomTabNavigator,
  type NativeBottomTabNavigationOptions,
  type NativeBottomTabNavigationProp,
} from "@react-navigation/bottom-tabs/unstable";
import type { NativeStackNavigationProp } from "@react-navigation/native-stack";
import { useTranslation } from "react-i18next";
import { useTheme } from "@/hooks/useTheme";
import { HomeView } from "@/screens/HomeView";
import { SettingsScreen } from "@/screens/SettingsScreen";
import { DevicesScreen } from "@/screens/DevicesScreen";
import type {
  MainTabParamList,
  RootStackParamList,
} from "./AppNavigator.types";

/** iOS 标签栏多一个系统搜索圆钮;它只是入口,不可选中,按下时打开剪贴板的搜索视图。 */
type IosMainTabParamList = MainTabParamList & { Search: undefined };

const Tab = createNativeBottomTabNavigator<IosMainTabParamList>();

type ClipboardNavigation = CompositeNavigationProp<
  NativeBottomTabNavigationProp<IosMainTabParamList, "Clipboard">,
  NativeStackNavigationProp<RootStackParamList>
>;

function ClipboardDestination({
  searchRequestId,
  onImmersiveModeChange,
}: {
  searchRequestId: number;
  onImmersiveModeChange: (immersive: boolean) => void;
}) {
  const navigation = useNavigation<ClipboardNavigation>();
  const openSettings = useCallback(
    () => navigation.navigate("Preferences"),
    [navigation],
  );
  const openAbout = useCallback(
    () => navigation.navigate("Preferences"),
    [navigation],
  );
  return (
    <HomeView
      onOpenSettings={openSettings}
      onOpenAbout={openAbout}
      onImmersiveModeChange={onImmersiveModeChange}
      searchRequestId={searchRequestId}
    />
  );
}

/** 搜索标签不可选中,永远不会显示内容 */
function SearchEntry() {
  return null;
}

/**
 * iOS 主页面:顶级目的地 剪贴板 / 设备 / 设置,底部是原生 UITabBarController 标签栏
 * (iOS 26 上即系统 Liquid Glass 标签栏)+ 系统搜索圆钮。
 * 搜索只作用于剪贴板历史:搜索圆钮不可选中,按下时切到剪贴板并进入搜索。首页搜索 / 多选时
 * 隐藏标签栏,底部让给搜索框与多选操作栏。
 */
export function MainScreen() {
  const { t } = useTranslation("home");
  const { theme } = useTheme();
  const [homeImmersive, setHomeImmersive] = useState(false);
  const [searchRequestId, setSearchRequestId] = useState(0);

  const screenOptions = useMemo<NativeBottomTabNavigationOptions>(
    () => ({
      headerShown: false,
      lazy: true,
      // 不让原生 tabs 改写首个 ScrollView 的 inset:SwiftUI 表单自己避开 safe area,
      // 首页列表自己留底部空间(HomeView tabBarClearance),否则会叠加两份
      overrideScrollViewContentInsetAdjustmentBehavior: false,
      tabBarActiveTintColor: theme.colors.accent,
    }),
    [theme.colors.accent],
  );

  return (
    <Tab.Navigator screenOptions={screenOptions}>
      <Tab.Screen
        name="Clipboard"
        options={{
          title: t("nav.clipboard"),
          tabBarIcon: { type: "sfSymbol", name: "doc.on.clipboard" },
          tabBarStyle: { display: homeImmersive ? "none" : "flex" },
        }}
      >
        {() => (
          <ClipboardDestination
            searchRequestId={searchRequestId}
            onImmersiveModeChange={setHomeImmersive}
          />
        )}
      </Tab.Screen>
      <Tab.Screen
        name="Devices"
        options={{
          title: t("nav.devices"),
          tabBarIcon: { type: "sfSymbol", name: "laptopcomputer.and.iphone" },
        }}
      >
        {({ route }) => <DevicesScreen {...route.params} />}
      </Tab.Screen>
      <Tab.Screen
        name="Preferences"
        options={{
          title: t("nav.settings"),
          tabBarIcon: { type: "sfSymbol", name: "gearshape" },
        }}
      >
        {() => <SettingsScreen />}
      </Tab.Screen>
      <Tab.Screen
        name="Search"
        component={SearchEntry}
        options={{ tabBarSystemItem: "search", tabBarSelectionEnabled: false }}
        listeners={({ navigation }) => ({
          tabPress: () => {
            navigation.navigate("Clipboard");
            setSearchRequestId((id) => id + 1);
          },
        })}
      />
    </Tab.Navigator>
  );
}
