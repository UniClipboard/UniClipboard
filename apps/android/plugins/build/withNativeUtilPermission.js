"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
const config_plugins_1 = require("expo/config-plugins");
/**
 * Adds POST_NOTIFICATIONS permission to AndroidManifest.xml
 * Required by NativeUtilModule for debug notifications and foreground service on Android 13+
 */
function addNativeUtilPermission(androidManifest) {
    const { manifest } = androidManifest;
    if (!manifest['uses-permission']) {
        manifest['uses-permission'] = [];
    }
    const requiredPermissions = [
        'android.permission.POST_NOTIFICATIONS',
        'android.permission.REQUEST_IGNORE_BATTERY_OPTIMIZATIONS',
    ];
    for (const perm of requiredPermissions) {
        const exists = manifest['uses-permission'].some((p) => p.$?.['android:name'] === perm);
        if (!exists) {
            manifest['uses-permission'].push({
                $: { 'android:name': perm },
            });
            console.log(`✓ Added permission: ${perm}`);
        }
    }
    return androidManifest;
}
const withNativeUtilPermission = (config) => {
    return (0, config_plugins_1.withAndroidManifest)(config, (config) => {
        config.modResults = addNativeUtilPermission(config.modResults);
        return config;
    });
};
exports.default = (0, config_plugins_1.createRunOncePlugin)(withNativeUtilPermission, 'withNativeUtilPermission', '1.0.0');
