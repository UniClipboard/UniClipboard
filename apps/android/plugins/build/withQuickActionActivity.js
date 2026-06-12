"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
const config_plugins_1 = require("expo/config-plugins");
/**
 * Adds QuickActionActivity to AndroidManifest.xml.
 * This transparent Activity handles quick clipboard download/upload
 * without showing the main app UI.
 */
function addQuickActionActivity(androidManifest) {
    const { manifest } = androidManifest;
    if (!Array.isArray(manifest.application)) {
        console.warn('withQuickActionActivity: No application array in manifest?');
        return androidManifest;
    }
    const application = manifest.application[0];
    if (!application.activity) {
        application.activity = [];
    }
    const activityName = '.quickaction.QuickActionActivity';
    const existingIndex = application.activity.findIndex((a) => a.$['android:name'] === activityName);
    const activityEntry = {
        $: {
            'android:name': activityName,
            'android:exported': 'true',
            'android:theme': '@style/Theme.QuickAction.Transparent',
            'android:taskAffinity': '',
            'android:excludeFromRecents': 'true',
            'android:launchMode': 'singleTask',
            'android:screenOrientation': 'portrait',
            'android:configChanges': 'keyboard|keyboardHidden|orientation|screenSize|screenLayout|uiMode|smallestScreenSize',
        },
    };
    if (existingIndex >= 0) {
        application.activity[existingIndex] = activityEntry;
    }
    else {
        application.activity.push(activityEntry);
    }
    return androidManifest;
}
const withQuickActionActivity = (config) => {
    return (0, config_plugins_1.withAndroidManifest)(config, (config) => {
        config.modResults = addQuickActionActivity(config.modResults);
        return config;
    });
};
exports.default = (0, config_plugins_1.createRunOncePlugin)(withQuickActionActivity, 'withQuickActionActivity', '1.0.0');
