import {
  AndroidConfig,
  ConfigPlugin,
  withAndroidManifest,
  createRunOncePlugin,
} from 'expo/config-plugins';

/**
 * Adds RECEIVE_SMS permission to AndroidManifest.xml
 * Required by SmsForwarderModule for listening to incoming SMS messages
 */
function addSmsPermission(
  androidManifest: AndroidConfig.Manifest.AndroidManifest
): AndroidConfig.Manifest.AndroidManifest {
  const { manifest } = androidManifest;

  if (!manifest['uses-permission']) {
    manifest['uses-permission'] = [];
  }

  const requiredPermissions = ['android.permission.RECEIVE_SMS', 'android.permission.READ_SMS'];

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

const withSmsPermission: ConfigPlugin = (config) => {
  return withAndroidManifest(config, (config) => {
    config.modResults = addSmsPermission(config.modResults);
    return config;
  });
};

export default createRunOncePlugin(withSmsPermission, 'withSmsPermission', '1.0.0');
