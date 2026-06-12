"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
const config_plugins_1 = require("expo/config-plugins");
const withForegroundService = (config) => {
    return (0, config_plugins_1.withAndroidManifest)(config, (modConfig) => {
        const manifest = modConfig.modResults.manifest;
        // Add FOREGROUND_SERVICE permission
        if (!manifest['uses-permission']) {
            manifest['uses-permission'] = [];
        }
        const permissions = manifest['uses-permission'];
        const addPermission = (name) => {
            if (!permissions.some((p) => p.$?.['android:name'] === name)) {
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
            if (!application.service.some((s) => s.$?.['android:name'] === serviceClassName)) {
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
exports.default = withForegroundService;
