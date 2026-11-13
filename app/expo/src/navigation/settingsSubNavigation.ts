import type { NativeStackNavigationProp } from '@react-navigation/native-stack';

import type { RootStackParamList } from './AppNavigator.types';

type SettingsSubParams = RootStackParamList['SettingsSub'];

/**
 * Opens an Android settings sub page. Always pushes: `SettingsSub` is one route name, so
 * `navigate` from a page that is already `SettingsSub` only swaps its params. The page would
 * be replaced instead of stacked, and back would skip a level.
 */
export function pushSettingsSub(
  navigation: Pick<NativeStackNavigationProp<RootStackParamList>, 'push'>,
  params: SettingsSubParams
): void {
  navigation.push('SettingsSub', params);
}
