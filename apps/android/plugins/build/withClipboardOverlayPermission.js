"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
const config_plugins_1 = require("expo/config-plugins");
/**
 * Adds SYSTEM_ALERT_WINDOW permission to AndroidManifest.xml
 * SYSTEM_ALERT_WINDOW: Required for the clipboard overlay feature
 */
function addOverlayPermission(androidManifest) {
    const { manifest } = androidManifest;
    if (!manifest['uses-permission']) {
        manifest['uses-permission'] = [];
    }
    const requiredPermissions = ['android.permission.SYSTEM_ALERT_WINDOW'];
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
const withClipboardOverlayPermission = (config) => {
    return (0, config_plugins_1.withAndroidManifest)(config, (config) => {
        config.modResults = addOverlayPermission(config.modResults);
        return config;
    });
};
exports.default = (0, config_plugins_1.createRunOncePlugin)(withClipboardOverlayPermission, 'withClipboardOverlayPermission', '1.0.0');
