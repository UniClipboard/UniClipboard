/**
 * App Navigation
 */

import React from 'react';
import { NavigationContainer, DefaultTheme, DarkTheme } from '@react-navigation/native';
import { createBottomTabNavigator } from '@react-navigation/bottom-tabs';
import { navigationRef } from './navigationRef';
import { Ionicons } from '@expo/vector-icons';
import { useTheme } from '@/hooks/useTheme';
import { HomeScreen } from '@/screens/HomeScreen';
import { HistoryScreen } from '@/screens/HistoryScreen';
import { SettingsScreen } from '@/screens/SettingsScreen';

const Tab = createBottomTabNavigator();

export const AppNavigator = () => {
  const { theme } = useTheme();

  // 创建适应主题的导航主题
  const navigationTheme = theme.isDark
    ? {
        ...DarkTheme,
        colors: {
          ...DarkTheme.colors,
          primary: theme.colors.primary,
          background: theme.colors.background,
          card: theme.colors.surface,
          text: theme.colors.text,
          border: theme.colors.border,
        },
      }
    : {
        ...DefaultTheme,
        colors: {
          ...DefaultTheme.colors,
          primary: theme.colors.primary,
          background: theme.colors.background,
          card: theme.colors.surface,
          text: theme.colors.text,
          border: theme.colors.border,
        },
      };

  return (
    <NavigationContainer ref={navigationRef} theme={navigationTheme}>
      <Tab.Navigator
        screenOptions={({ route }) => ({
          headerStyle: {
            backgroundColor: theme.colors.surface,
            elevation: 0,
            shadowOpacity: 0,
            borderBottomWidth: 0,
          },
          headerTintColor: theme.colors.text,
          headerTitleStyle: {
            fontWeight: 'bold',
          },
          tabBarStyle: {
            backgroundColor: theme.colors.tabBarBackground,
            borderTopColor: theme.colors.tabBarBorder,
            borderTopWidth: 1,
            elevation: 0,
            shadowOpacity: 0,
          },
          tabBarActiveTintColor: theme.colors.tabBarActive,
          tabBarInactiveTintColor: theme.colors.tabBarInactive,
          tabBarIcon: ({ color, size }) => {
            let iconName = 'home';
            if (route.name === 'History') {
              iconName = 'time';
            } else if (route.name === 'Settings') {
              iconName = 'settings';
            }
            return (
              <Ionicons
                name={iconName as keyof typeof Ionicons.glyphMap}
                size={size}
                color={color}
              />
            );
          },
        })}
      >
        <Tab.Screen name="Home" component={HomeScreen} options={{ title: '首页' }} />
        <Tab.Screen name="History" component={HistoryScreen} options={{ title: '历史' }} />
        <Tab.Screen name="Settings" component={SettingsScreen} options={{ title: '设置' }} />
      </Tab.Navigator>
    </NavigationContainer>
  );
};
