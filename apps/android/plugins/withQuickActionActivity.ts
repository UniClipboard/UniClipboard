import {
  AndroidConfig,
  ConfigPlugin,
  withAndroidManifest,
  createRunOncePlugin,
} from 'expo/config-plugins';

/**
 * Adds QuickActionActivity to AndroidManifest.xml.
 * This transparent Activity handles quick clipboard download/upload
 * without showing the main app UI.
 */
function addQuickActionActivity(
  androidManifest: AndroidConfig.Manifest.AndroidManifest
): AndroidConfig.Manifest.AndroidManifest {
  const { manifest } = androidManifest;

  if (!Array.isArray(manifest.application)) {
    console.warn('withQuickActionActivity: No application array in manifest?');
    return androidManifest;
  }

  const application = manifest.application[0];

  if (!application.activity) {
    application.activity = [];
  }

  const activityName = '.quickaction.QuickActionActivity';

  type ManifestActivity = (typeof application.activity)[0];

  const existingIndex = application.activity.findIndex(
    (a) => (a as { $: { 'android:name': string } }).$['android:name'] === activityName
  );

  const activityEntry = {
    $: {
      'android:name': activityName,
      'android:exported': 'true',
      'android:theme': '@style/Theme.QuickAction.Transparent',
      'android:taskAffinity': '',
      'android:excludeFromRecents': 'true',
      'android:launchMode': 'singleTask',
      'android:screenOrientation': 'portrait',
      'android:configChanges':
        'keyboard|keyboardHidden|orientation|screenSize|screenLayout|uiMode|smallestScreenSize',
    },
  };

  if (existingIndex >= 0) {
    application.activity[existingIndex] = activityEntry as unknown as ManifestActivity;
  } else {
    application.activity.push(activityEntry as unknown as ManifestActivity);
  }

  return androidManifest;
}

const withQuickActionActivity: ConfigPlugin = (config) => {
  return withAndroidManifest(config, (config) => {
    config.modResults = addQuickActionActivity(config.modResults);
    return config;
  });
};

export default createRunOncePlugin(withQuickActionActivity, 'withQuickActionActivity', '1.0.0');
