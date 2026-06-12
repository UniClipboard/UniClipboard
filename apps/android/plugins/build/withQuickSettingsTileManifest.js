"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
const config_plugins_1 = require("expo/config-plugins");
/**
 * Adds Quick Settings Tile service to AndroidManifest.xml
 */
function addQuickSettingsTileService(androidManifest) {
    const { manifest } = androidManifest;
    if (!Array.isArray(manifest.application)) {
        console.warn('withQuickSettingsTile: No application array in manifest?');
        return androidManifest;
    }
    const application = manifest.application[0];
    if (!application.service) {
        application.service = [];
    }
    const services = [
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
        const existingIndex = application.service.findIndex((s) => s.$['android:name'] === name);
        if (existingIndex >= 0) {
            // Update existing service attributes to ensure correctness
            application.service[existingIndex] = service;
        }
        else {
            application.service.push(service);
        }
    }
    return androidManifest;
}
/**
 * Plugin to add Quick Settings Tile service to AndroidManifest
 */
const withQuickSettingsTileManifest = (config) => {
    return (0, config_plugins_1.withAndroidManifest)(config, (config) => {
        config.modResults = addQuickSettingsTileService(config.modResults);
        return config;
    });
};
exports.default = (0, config_plugins_1.createRunOncePlugin)(withQuickSettingsTileManifest, 'withQuickSettingsTileManifest', '1.0.0');
