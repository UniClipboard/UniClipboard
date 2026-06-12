import {
  ConfigPlugin,
  withAndroidManifest,
  withAppBuildGradle,
  createRunOncePlugin,
} from 'expo/config-plugins';

const SHIZUKU_VERSION = '13.1.5';

/**
 * Adds Shizuku provider to AndroidManifest.xml and Shizuku dependencies to app build.gradle.
 * The provider class lives in the shizuku:provider artifact, which must be a direct
 * dependency of the app module so it ends up in the final APK's dex.
 */
const withShizukuProvider: ConfigPlugin = (config) => {
  // 1. Inject Shizuku dependencies into app/build.gradle
  config = withAppBuildGradle(config, (modConfig) => {
    const depLine = `    implementation "dev.rikka.shizuku:provider:${SHIZUKU_VERSION}"`;
    if (!modConfig.modResults.contents.includes('dev.rikka.shizuku:provider')) {
      // Find the dependencies block and append inside it
      modConfig.modResults.contents = modConfig.modResults.contents.replace(
        /dependencies\s*\{/,
        `dependencies {\n${depLine}`
      );
      console.log('✓ Added Shizuku provider dependency to app build.gradle');
    }
    return modConfig;
  });

  // 2. Register ShizukuProvider in AndroidManifest.xml
  config = withAndroidManifest(config, (modConfig) => {
    const manifest = modConfig.modResults.manifest;
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const application = manifest.application?.[0] as Record<string, any> | undefined;

    if (!application) {
      return modConfig;
    }

    if (!application.provider) {
      application.provider = [];
    }

    // 添加 ShizukuProvider（Shizuku API 必需）
    const shizukuProviderName = 'rikka.shizuku.ShizukuProvider';
    const hasProvider = application.provider.some(
      (p: { $?: { 'android:name'?: string } }) => p.$?.['android:name'] === shizukuProviderName
    );

    if (!hasProvider) {
      const packageName = modConfig.android?.package || 'app.uniclipboard.android';

      application.provider.push({
        $: {
          'android:name': shizukuProviderName,
          'android:authorities': `${packageName}.shizuku`,
          'android:multiprocess': 'false',
          'android:enabled': 'true',
          'android:exported': 'true',
          'android:permission': 'android.permission.INTERACT_ACROSS_USERS_FULL',
        },
      });
      console.log('✓ Added ShizukuProvider to AndroidManifest.xml');
    }

    return modConfig;
  });

  return config;
};

export default createRunOncePlugin(withShizukuProvider, 'withShizukuProvider', '1.0.0');
