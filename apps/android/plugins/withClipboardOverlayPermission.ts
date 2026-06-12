import {
  AndroidConfig,
  ConfigPlugin,
  withAndroidManifest,
  createRunOncePlugin,
} from 'expo/config-plugins';

/**
 * Adds SYSTEM_ALERT_WINDOW permission to AndroidManifest.xml
 * SYSTEM_ALERT_WINDOW: Required for the clipboard overlay feature
 */
function addOverlayPermission(
  androidManifest: AndroidConfig.Manifest.AndroidManifest
): AndroidConfig.Manifest.AndroidManifest {
  const { manifest } = androidManifest;

  if (!manifest['uses-permission']) {
    manifest['uses-permission'] = [];
  }

  const requiredPermissions = ['android.permission.SYSTEM_ALERT_WINDOW'];

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

const withClipboardOverlayPermission: ConfigPlugin = (config) => {
  return withAndroidManifest(config, (config) => {
    config.modResults = addOverlayPermission(config.modResults);
    return config;
  });
};

export default createRunOncePlugin(
  withClipboardOverlayPermission,
  'withClipboardOverlayPermission',
  '1.0.0'
);
