import { useCallback, useMemo, useState, type ReactNode } from 'react';
import { useWindowDimensions } from 'react-native';
import {
  useNavigation,
  type CompositeNavigationProp,
  type RouteProp,
} from '@react-navigation/native';
import {
  createBottomTabNavigator,
  type BottomTabBarProps,
  type BottomTabNavigationOptions,
  type BottomTabNavigationProp,
} from '@react-navigation/bottom-tabs';
import type { NativeStackNavigationProp } from '@react-navigation/native-stack';
import { useSharedValue, type SharedValue } from 'react-native-reanimated';
import { SafeAreaInsetsContext, useSafeAreaInsets } from 'react-native-safe-area-context';
import { useTranslation } from 'react-i18next';
import { pushSettingsSub } from './settingsSubNavigation';
import { FAB_SIZE } from '@/components/AddActionsFab.types';
import { MainNavigationBar } from '@/components/android/MainNavigationBar';
import {
  FLOATING_NAV_CLEARANCE,
  FLOATING_NAV_HEIGHT,
  FLOATING_NAV_MARGIN,
} from '@/components/android/mainNavigationMetrics';
import { FloatingNavigationClearanceContext } from '@/components/android/floatingNavigationClearance';
import { getLayoutMode } from '@/hooks/useLayoutMode';
import { useTheme } from '@/hooks/useTheme';
import { HomeView } from '@/screens/HomeView';
import { SettingsScreen } from '@/screens/SettingsScreen';
import { DevicesScreen } from '@/screens/DevicesScreen';

import type { MainTabParamList, RootStackParamList } from './AppNavigator.types';

const Tab = createBottomTabNavigator<MainTabParamList>();

/**
 * 首页 FAB 底边 = bottom inset + 12。把胶囊高度差计入首页的 bottom inset,FAB 即与
 * 悬浮导航胶囊垂直居中对齐;Snackbar(FAB 之上)随之落在胶囊上方。
 */
const HOME_FAB_EDGE = 12;
const HOME_FLOATING_NAV_LIFT =
  FLOATING_NAV_MARGIN + (FLOATING_NAV_HEIGHT - FAB_SIZE) / 2 - HOME_FAB_EDGE;

/** 首页的 FAB / Snackbar / 网格都按 bottom inset 排布:显示胶囊时把抬升量计入 inset。 */
function HomeFloatingNavigationInsets({ lift, children }: { lift: number; children: ReactNode }) {
  const insets = useSafeAreaInsets();
  const value = useMemo(
    () => (lift > 0 ? { ...insets, bottom: insets.bottom + lift } : insets),
    [insets, lift]
  );
  return <SafeAreaInsetsContext.Provider value={value}>{children}</SafeAreaInsetsContext.Provider>;
}

type ClipboardNavigation = CompositeNavigationProp<
  BottomTabNavigationProp<MainTabParamList, 'Clipboard'>,
  NativeStackNavigationProp<RootStackParamList>
>;

function ClipboardDestination({
  navigationBarShown,
  onImmersiveModeChange,
  addMenuOpenSignal,
}: {
  navigationBarShown: boolean;
  onImmersiveModeChange: (immersive: boolean) => void;
  addMenuOpenSignal: SharedValue<boolean>;
}) {
  const navigation = useNavigation<ClipboardNavigation>();
  const openSettings = useCallback(() => navigation.navigate('Preferences'), [navigation]);
  const openAbout = useCallback(
    () => pushSettingsSub(navigation, { section: 'about' }),
    [navigation]
  );
  return (
    <HomeFloatingNavigationInsets lift={navigationBarShown ? HOME_FLOATING_NAV_LIFT : 0}>
      <HomeView
        onOpenSettings={openSettings}
        onOpenAbout={openAbout}
        onImmersiveModeChange={onImmersiveModeChange}
        addMenuOpenSignal={addMenuOpenSignal}
      />
    </HomeFloatingNavigationInsets>
  );
}

function DevicesDestination({
  route,
  navigationBarShown,
}: {
  route: RouteProp<MainTabParamList, 'Devices'>;
  navigationBarShown: boolean;
}) {
  return (
    <FloatingNavigationClearanceContext.Provider
      value={navigationBarShown ? FLOATING_NAV_CLEARANCE : 0}
    >
      <DevicesScreen {...route.params} />
    </FloatingNavigationClearanceContext.Provider>
  );
}

function PreferencesDestination({ navigationBarShown }: { navigationBarShown: boolean }) {
  return (
    <FloatingNavigationClearanceContext.Provider
      value={navigationBarShown ? FLOATING_NAV_CLEARANCE : 0}
    >
      <SettingsScreen />
    </FloatingNavigationClearanceContext.Provider>
  );
}

/**
 * Android 主页面:顶级目的地 剪贴板 / 设备 / 设置。手机用左下悬浮导航胶囊(与首页 FAB
 * 同一水平线),平板(expanded)改为左侧 navigation rail。首页搜索 / 多选时隐藏胶囊,
 * 把底部让给键盘与多选操作栏。
 * 设置二级页仍 push 到根 Stack,覆盖整个主页面。
 */
export function MainScreen() {
  const { t } = useTranslation('home');
  const { theme } = useTheme();
  const { width } = useWindowDimensions();
  const useRail = getLayoutMode(width) === 'expanded';
  const [homeImmersive, setHomeImmersive] = useState(false);
  // 添加菜单展开时胶囊让位于菜单遮罩,但不改首页 inset,FAB 位置不动。FAB 点按即写入
  // 这个 shared value,胶囊在 UI 线程上直接响应,不经 React 重渲
  const homeAddMenuOpen = useSharedValue(false);
  const navigationBarShown = !useRail && !homeImmersive;
  // 设备 / 设置页没有沉浸态,胶囊在手机上恒显示
  const floatingNavShown = !useRail;

  const renderTabBar = useCallback(
    (props: BottomTabBarProps) => (
      <MainNavigationBar {...props} hidden={homeImmersive} hiddenByMenu={homeAddMenuOpen} />
    ),
    [homeAddMenuOpen, homeImmersive]
  );

  const screenOptions = useMemo<BottomTabNavigationOptions>(
    () => ({
      tabBarPosition: useRail ? 'left' : 'bottom',
      headerShown: false,
      sceneStyle: { backgroundColor: theme.colors.background as string },
      headerStyle: { backgroundColor: theme.colors.background as string },
      headerShadowVisible: false,
      headerTintColor: theme.colors.textPrimary as string,
    }),
    [theme.colors.background, theme.colors.textPrimary, useRail]
  );

  return (
    <Tab.Navigator tabBar={renderTabBar} screenOptions={screenOptions}>
      <Tab.Screen name="Clipboard" options={{ title: t('nav.clipboard') }}>
        {() => (
          <ClipboardDestination
            navigationBarShown={navigationBarShown}
            onImmersiveModeChange={setHomeImmersive}
            addMenuOpenSignal={homeAddMenuOpen}
          />
        )}
      </Tab.Screen>
      <Tab.Screen name="Devices" options={{ title: t('nav.devices') }}>
        {({ route }) => <DevicesDestination route={route} navigationBarShown={floatingNavShown} />}
      </Tab.Screen>
      <Tab.Screen name="Preferences" options={{ title: t('nav.settings') }}>
        {() => <PreferencesDestination navigationBarShown={floatingNavShown} />}
      </Tab.Screen>
    </Tab.Navigator>
  );
}
