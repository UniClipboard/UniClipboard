import type { NavigatorScreenParams } from '@react-navigation/native';


export type SettingsSubSection =
  | 'syncChannel'
  | 'space'
  | 'spaceSettings'
  | 'relay'
  | 'relayEditor'
  | 'lanServers'
  | 'keyboard'
  | 'share'
  | 'clipboard'
  | 'history'
  | 'background'
  | 'storage'
  | 'privacy'
  | 'diagnostics'
  | 'about'
  | 'developer';

/** Target of a notification or deep link that opens one space device (or the device list). */
export type SpaceDeviceTarget = {
  deviceId?: string;
  notificationNavigationRequestId?: number;
};

/** Android top-level destinations hosted by the Main bottom navigation. */
export type MainTabParamList = {
  Clipboard: undefined;
  Devices: SpaceDeviceTarget | undefined;
  Preferences: undefined;
};

export type RootStackParamList = {
  Onboarding: undefined;
  OnboardingPreview: undefined;
  ConnectionPreview: undefined;
  Main: NavigatorScreenParams<MainTabParamList> | undefined;
  Settings:
    | {
        section?: 'space' | 'lanServers';
        deviceId?: string;
        notificationNavigationRequestId?: number;
      }
    | undefined;
  SettingsSub: {
    section: SettingsSubSection;

    deviceId?: string;
    notificationNavigationRequestId?: number;
    /** relayEditor: the relay being edited; absent when adding a new one. */
    relayUrl?: string;
  };
};
