import { ConfigPlugin, withAndroidManifest } from 'expo/config-plugins';

const withForegroundService: ConfigPlugin = (config) => {
  return withAndroidManifest(config, (modConfig) => {
    const manifest = modConfig.modResults.manifest;

    // Add FOREGROUND_SERVICE permission
    if (!manifest['uses-permission']) {
      manifest['uses-permission'] = [];
    }
    const permissions = manifest['uses-permission'];

    const addPermission = (name: string) => {
      if (
        !permissions.some(
          (p: { $?: { 'android:name'?: string } }) => p.$?.['android:name'] === name
        )
      ) {
        permissions.push({ $: { 'android:name': name } });
        console.log(`✅ Added permission: ${name}`);
      }
    };

    addPermission('android.permission.FOREGROUND_SERVICE');
    addPermission('android.permission.FOREGROUND_SERVICE_DATA_SYNC');

    // Register Service in <application>
    const application = manifest.application?.[0];
    if (application) {
      if (!application.service) {
        application.service = [];
      }
      const serviceClassName = 'expo.modules.foregroundservice.SyncForegroundService';
      if (
        !application.service.some(
          (s: { $?: { 'android:name'?: string } }) => s.$?.['android:name'] === serviceClassName
        )
      ) {
        application.service.push({
          $: {
            'android:name': serviceClassName,
            'android:enabled': 'true',
            'android:exported': 'false',
            'android:foregroundServiceType': 'dataSync',
          },
        });
        console.log(`✅ Registered service: ${serviceClassName}`);
      }
    }

    return modConfig;
  });
};

export default withForegroundService;
