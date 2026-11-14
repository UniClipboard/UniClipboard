/**
 * App Constants
 */

import * as Application from 'expo-application';

export const APP_NAME = 'UniClipboard UI Shell';
export const APP_VERSION = Application.nativeApplicationVersion ?? '0.0.0';
export const APP_BUILD_NUMBER = Application.nativeBuildVersion;
export const APP_VERSION_WITH_BUILD = APP_BUILD_NUMBER
  ? `${APP_VERSION} (${APP_BUILD_NUMBER})`
  : APP_VERSION;
