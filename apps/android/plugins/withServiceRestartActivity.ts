import {
  AndroidConfig,
  ConfigPlugin,
  withAndroidManifest,
  createRunOncePlugin,
} from 'expo/config-plugins';

/**
 * Adds ServiceRestartActivity to AndroidManifest.xml.
 * This transparent Activity is launched when the foreground service is restarted
 * by the system (START_STICKY) without a JS runtime. It briefly shows a success
 * overlay and boots the JS runtime.
 */
function addServiceRestartActivity(
  androidManifest: AndroidConfig.Manifest.AndroidManifest
): AndroidConfig.Manifest.AndroidManifest {
  const { manifest } = androidManifest;

  if (!Array.isArray(manifest.application)) {
    console.warn('withServiceRestartActivity: No application array in manifest?');
    return androidManifest;
  }

  const application = manifest.application[0];

  if (!application.activity) {
    application.activity = [];
  }

  const activityName = '.servicerestart.ServiceRestartActivity';

  type ManifestActivity = (typeof application.activity)[0];

  const existingIndex = application.activity.findIndex(
    (a) => (a as { $: { 'android:name': string } }).$['android:name'] === activityName
  );

  const activityEntry = {
    $: {
      'android:name': activityName,
      'android:exported': 'false',
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

const withServiceRestartActivity: ConfigPlugin = (config) => {
  return withAndroidManifest(config, (config) => {
    config.modResults = addServiceRestartActivity(config.modResults);
    return config;
  });
};

export default createRunOncePlugin(
  withServiceRestartActivity,
  'withServiceRestartActivity',
  '1.0.0'
);
