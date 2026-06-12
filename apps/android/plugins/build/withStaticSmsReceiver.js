"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
const config_plugins_1 = require("expo/config-plugins");
/**
 * 在 AndroidManifest.xml 中注册静态 SMS BroadcastReceiver。
 * 静态 Receiver 即使 app 被杀或 Doze 模式下仍能接收短信（SMS_RECEIVED 属于隐式广播豁免列表）。
 */
const withStaticSmsReceiver = (config) => {
    return (0, config_plugins_1.withAndroidManifest)(config, (modConfig) => {
        const manifest = modConfig.modResults.manifest;
        const application = manifest.application?.[0];
        if (!application)
            return modConfig;
        if (!application.receiver) {
            application.receiver = [];
        }
        const receiverClassName = 'expo.modules.smsforwarder.StaticSmsReceiver';
        const exists = application.receiver.some((r) => r.$?.['android:name'] === receiverClassName);
        if (!exists) {
            application.receiver.push({
                $: {
                    'android:name': receiverClassName,
                    'android:enabled': 'true',
                    'android:exported': 'true',
                    'android:permission': 'android.permission.BROADCAST_SMS',
                },
                'intent-filter': [
                    {
                        $: {
                            'android:priority': '2147483647',
                        },
                        action: [
                            {
                                $: { 'android:name': 'android.provider.Telephony.SMS_RECEIVED' },
                            },
                        ],
                    },
                ],
            });
            console.log(`✅ Registered static SMS receiver: ${receiverClassName}`);
        }
        return modConfig;
    });
};
exports.default = withStaticSmsReceiver;
