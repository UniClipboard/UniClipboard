import {
  AndroidConfig,
  ConfigPlugin,
  withAndroidManifest,
  createRunOncePlugin,
} from 'expo/config-plugins';

/**
 * Adds POST_NOTIFICATIONS permission to AndroidManifest.xml
 * Required by NativeUtilModule for debug notifications and foreground service on Android 13+
 */
function addNativeUtilPermission(
  androidManifest: AndroidConfig.Manifest.AndroidManifest
): AndroidConfig.Manifest.AndroidManifest {
  const { manifest } = androidManifest;

  if (!manifest['uses-permission']) {
    manifest['uses-permission'] = [];
  }

  const requiredPermissions = [
    'android.permission.POST_NOTIFICATIONS',
    'android.permission.REQUEST_IGNORE_BATTERY_OPTIMIZATIONS',
  ];

  for (const perm of requiredPermissions) {
    const exists = manifest['uses-permission'].some((p) => p.$?.['android:name'] === perm);
    if (!exists) {
      manifest['uses-permission'].push({
        $: { 'android:name': perm },
      } as NonNullable<(typeof manifest)['uses-permission']>[0]);
      console.log(`✓ Added permission: ${perm}`);
    }
  }

  return androidManifest;
}

const withNativeUtilPermission: ConfigPlugin = (config) => {
  return withAndroidManifest(config, (config) => {
    config.modResults = addNativeUtilPermission(config.modResults);
    return config;
  });
};

export default createRunOncePlugin(withNativeUtilPermission, 'withNativeUtilPermission', '1.0.0');
