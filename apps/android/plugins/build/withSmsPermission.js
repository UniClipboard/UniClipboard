"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
const config_plugins_1 = require("expo/config-plugins");
/**
 * Adds RECEIVE_SMS permission to AndroidManifest.xml
 * Required by SmsForwarderModule for listening to incoming SMS messages
 */
function addSmsPermission(androidManifest) {
    const { manifest } = androidManifest;
    if (!manifest['uses-permission']) {
        manifest['uses-permission'] = [];
    }
    const requiredPermissions = ['android.permission.RECEIVE_SMS', 'android.permission.READ_SMS'];
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
const withSmsPermission = (config) => {
    return (0, config_plugins_1.withAndroidManifest)(config, (config) => {
        config.modResults = addSmsPermission(config.modResults);
        return config;
    });
};
exports.default = (0, config_plugins_1.createRunOncePlugin)(withSmsPermission, 'withSmsPermission', '1.0.0');
