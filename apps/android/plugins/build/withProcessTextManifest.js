"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
const config_plugins_1 = require("expo/config-plugins");
/**
 * Adds ProcessTextActivity to AndroidManifest.xml so that the app appears
 * in the Android floating text selection toolbar as "SyncClipboard".
 */
function addProcessTextActivity(androidManifest) {
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
    const existingIndex = application.activity.findIndex((a) => a.$['android:name'] === activityName);
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
        application.activity[existingIndex] = activityEntry;
    }
    else {
        application.activity.push(activityEntry);
    }
    return androidManifest;
}
const withProcessTextManifest = (config) => {
    return (0, config_plugins_1.withAndroidManifest)(config, (config) => {
        config.modResults = addProcessTextActivity(config.modResults);
        return config;
    });
};
exports.default = (0, config_plugins_1.createRunOncePlugin)(withProcessTextManifest, 'withProcessTextManifest', '1.0.0');
