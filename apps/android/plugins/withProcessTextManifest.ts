import {
  AndroidConfig,
  ConfigPlugin,
  withAndroidManifest,
  createRunOncePlugin,
} from 'expo/config-plugins';

/**
 * Adds ProcessTextActivity to AndroidManifest.xml so that the app appears
 * in the Android floating text selection toolbar as "SyncClipboard".
 */
function addProcessTextActivity(
  androidManifest: AndroidConfig.Manifest.AndroidManifest
): AndroidConfig.Manifest.AndroidManifest {
  const { manifest } = androidManifest;

  if (!Array.isArray(manifest.application)) {
    console.warn('withProcessTextManifest: No application array in manifest?');
    return androidManifest;
  }

  const application = manifest.application[0];

  if (!application.activity) {
    application.activity = [];
  }

  const activityName = '.processtext.ProcessTextActivity';

  type ManifestActivity = (typeof application.activity)[0];

  const existingIndex = application.activity.findIndex(
    (a) => (a as { $: { 'android:name': string } }).$['android:name'] === activityName
  );

  const activityEntry = {
    $: {
      'android:name': activityName,
      'android:label': '上传',
      'android:exported': 'true',
      'android:theme': '@android:style/Theme.NoDisplay',
      'android:taskAffinity': '',
      'android:excludeFromRecents': 'true',
    },
    'intent-filter': [
      {
        action: [{ $: { 'android:name': 'android.intent.action.PROCESS_TEXT' } }],
        category: [{ $: { 'android:name': 'android.intent.category.DEFAULT' } }],
        data: [{ $: { 'android:mimeType': 'text/plain' } }],
      },
    ],
  };

  if (existingIndex >= 0) {
    application.activity[existingIndex] = activityEntry as unknown as ManifestActivity;
  } else {
    application.activity.push(activityEntry as unknown as ManifestActivity);
  }

  return androidManifest;
}

const withProcessTextManifest: ConfigPlugin = (config) => {
  return withAndroidManifest(config, (config) => {
    config.modResults = addProcessTextActivity(config.modResults);
    return config;
  });
};

export default createRunOncePlugin(withProcessTextManifest, 'withProcessTextManifest', '1.0.0');
