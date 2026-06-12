"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
const config_plugins_1 = require("expo/config-plugins");
const IMPORT_LINE = 'import expo.modules.nativeutil.NativeLogger';
const INIT_CALL = '    NativeLogger.init(this)';
const withNativeLogger = (config) => {
    return (0, config_plugins_1.withMainApplication)(config, (mod) => {
        let contents = mod.modResults.contents;
        // Add import if not already present
        if (!contents.includes(IMPORT_LINE)) {
            // Insert after the last import block line
            contents = contents.replace(/(^import .+$)(?![\s\S]*^import )/m, `$1\n${IMPORT_LINE}`);
        }
        // Add NativeLogger.init(this) call inside onCreate() if not already present
        if (!contents.includes(INIT_CALL)) {
            contents = contents.replace(/(override fun onCreate\(\) \{\n\s+super\.onCreate\(\))/, `$1\n${INIT_CALL}`);
        }
        mod.modResults.contents = contents;
        return mod;
    });
};
exports.default = (0, config_plugins_1.createRunOncePlugin)(withNativeLogger, 'withNativeLogger', '1.0.0');
