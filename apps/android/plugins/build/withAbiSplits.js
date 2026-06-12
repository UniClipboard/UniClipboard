"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
const config_plugins_1 = require("expo/config-plugins");
/**
 * Configures Android ABI splits so that separate APKs are built for
 * each CPU architecture (arm64-v8a, armeabi-v7a, x86_64).
 *
 * All variants share the same versionCode (no per-ABI offset).
 */
const withAbiSplits = (config) => {
    return (0, config_plugins_1.withAppBuildGradle)(config, (config) => {
        const contents = config.modResults.contents;
        // --- 1. splits block ---
        const splitsConfig = `
    splits {
        abi {
            enable true
            reset()
            include "arm64-v8a", "armeabi-v7a", "x86_64"
            universalApk true
        }
    }
`;
        if (!contents.includes('splits {')) {
            const androidBlockMatch = contents.match(/^android\s*\{[\s\S]*?\n\}/m);
            if (androidBlockMatch) {
                const androidBlock = androidBlockMatch[0];
                const modified = androidBlock.replace(/\n\}$/, splitsConfig + '\n}');
                config.modResults.contents = contents.replace(androidBlock, modified);
                console.log('✓ Added splits configuration to build.gradle');
            }
        }
        else {
            console.log('ℹ splits already configured in build.gradle');
        }
        return config;
    });
};
exports.default = (0, config_plugins_1.createRunOncePlugin)(withAbiSplits, 'withAbiSplits', '1.0.0');
