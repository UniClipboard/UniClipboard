import {
  AndroidConfig,
  ConfigPlugin,
  withAndroidManifest,
  createRunOncePlugin,
} from 'expo/config-plugins';

interface ServiceConfig {
  $: {
    'android:name': string;
    'android:exported': string;
    'android:icon': string;
    'android:label': string;
    'android:permission': string;
  };
  'intent-filter': Array<{
    action: Array<{
      $: {
        'android:name': string;
      };
    }>;
  }>;
}

/**
 * Adds Quick Settings Tile service to AndroidManifest.xml
 */
function addQuickSettingsTileService(
  androidManifest: AndroidConfig.Manifest.AndroidManifest
): AndroidConfig.Manifest.AndroidManifest {
  const { manifest } = androidManifest;

  if (!Array.isArray(manifest.application)) {
    console.warn('withQuickSettingsTile: No application array in manifest?');
    return androidManifest;
  }

  const application = manifest.application[0];

  if (!application.service) {
    application.service = [];
  }

  const services: ServiceConfig[] = [
    {
      $: {
        'android:name': '.quicksettings.DownloadTileService',
        'android:exported': 'true',
        'android:icon': '@drawable/ic_tile_download',
        'android:label': '@string/tile_download_label',
        'android:permission': 'android.permission.BIND_QUICK_SETTINGS_TILE',
      },
      'intent-filter': [
        { action: [{ $: { 'android:name': 'android.service.quicksettings.action.QS_TILE' } }] },
      ],
    },
    {
      $: {
        'android:name': '.quicksettings.UploadTileService',
        'android:exported': 'true',
        'android:icon': '@drawable/ic_tile_upload',
        'android:label': '@string/tile_upload_label',
        'android:permission': 'android.permission.BIND_QUICK_SETTINGS_TILE',
      },
      'intent-filter': [
        { action: [{ $: { 'android:name': 'android.service.quicksettings.action.QS_TILE' } }] },
      ],
    },
  ];

  for (const service of services) {
    const name = service.$['android:name'];
    type ManifestService = (typeof application.service)[0];
    const existingIndex = application.service.findIndex(
      (s) => (s as ServiceConfig).$['android:name'] === name
    );
    if (existingIndex >= 0) {
      // Update existing service attributes to ensure correctness
      application.service[existingIndex] = service as unknown as ManifestService;
    } else {
      application.service.push(service as unknown as ManifestService);
    }
  }

  return androidManifest;
}

/**
 * Plugin to add Quick Settings Tile service to AndroidManifest
 */
const withQuickSettingsTileManifest: ConfigPlugin = (config) => {
  return withAndroidManifest(config, (config) => {
    config.modResults = addQuickSettingsTileService(config.modResults);
    return config;
  });
};

export default createRunOncePlugin(
  withQuickSettingsTileManifest,
  'withQuickSettingsTileManifest',
  '1.0.0'
);
