"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
const config_plugins_1 = require("expo/config-plugins");
const SHIZUKU_VERSION = '13.1.5';
/**
 * Adds Shizuku provider to AndroidManifest.xml and Shizuku dependencies to app build.gradle.
 * The provider class lives in the shizuku:provider artifact, which must be a direct
 * dependency of the app module so it ends up in the final APK's dex.
 */
const withShizukuProvider = (config) => {
    // 1. Inject Shizuku dependencies into app/build.gradle
    config = (0, config_plugins_1.withAppBuildGradle)(config, (modConfig) => {
        const depLine = `    implementation "dev.rikka.shizuku:provider:${SHIZUKU_VERSION}"`;
        if (!modConfig.modResults.contents.includes('dev.rikka.shizuku:provider')) {
            // Find the dependencies block and append inside it
            modConfig.modResults.contents = modConfig.modResults.contents.replace(/dependencies\s*\{/, `dependencies {\n${depLine}`);
            console.log('✓ Added Shizuku provider dependency to app build.gradle');
        }
        return modConfig;
    });
    // 2. Register ShizukuProvider in AndroidManifest.xml
    config = (0, config_plugins_1.withAndroidManifest)(config, (modConfig) => {
        const manifest = modConfig.modResults.manifest;
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        const application = manifest.application?.[0];
        if (!application) {
            return modConfig;
        }
        if (!application.provider) {
            application.provider = [];
        }
        // 添加 ShizukuProvider（Shizuku API 必需）
        const shizukuProviderName = 'rikka.shizuku.ShizukuProvider';
        const hasProvider = application.provider.some((p) => p.$?.['android:name'] === shizukuProviderName);
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
exports.default = (0, config_plugins_1.createRunOncePlugin)(withShizukuProvider, 'withShizukuProvider', '1.0.0');
