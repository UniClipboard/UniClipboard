"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
const config_plugins_1 = require("expo/config-plugins");
/**
 * Adds ServiceRestartActivity to AndroidManifest.xml.
 * This transparent Activity is launched when the foreground service is restarted
 * by the system (START_STICKY) without a JS runtime. It briefly shows a success
 * overlay and boots the JS runtime.
 */
function addServiceRestartActivity(androidManifest) {
    const { manifest } = androidManifest;
    if (!Array.isArray(manifest.application)) {
        console.warn('withServiceRestartActivity: No application array in manifest?');
        return androidManifest;
    }
    const application = manifest.application[0];
    if (!application.activity) {
        application.activity = [];
    }
    const activityName = '.servicerestart.ServiceRestartActivity';
    const existingIndex = application.activity.findIndex((a) => a.$['android:name'] === activityName);
    const activityEntry = {
        $: {
            'android:name': activityName,
            'android:exported': 'false',
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
const withServiceRestartActivity = (config) => {
    return (0, config_plugins_1.withAndroidManifest)(config, (config) => {
        config.modResults = addServiceRestartActivity(config.modResults);
        return config;
    });
};
exports.default = (0, config_plugins_1.createRunOncePlugin)(withServiceRestartActivity, 'withServiceRestartActivity', '1.0.0');
