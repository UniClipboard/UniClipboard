import { ConfigPlugin, withMainApplication, createRunOncePlugin } from 'expo/config-plugins';

const IMPORT_LINE = 'import expo.modules.nativeutil.NativeLogger';
const INIT_CALL = '    NativeLogger.init(this)';

const withNativeLogger: ConfigPlugin = (config) => {
  return withMainApplication(config, (mod) => {
    let contents = mod.modResults.contents;

    // Add import if not already present
    if (!contents.includes(IMPORT_LINE)) {
      // Insert after the last import block line
      contents = contents.replace(/(^import .+$)(?![\s\S]*^import )/m, `$1\n${IMPORT_LINE}`);
    }

    // Add NativeLogger.init(this) call inside onCreate() if not already present
    if (!contents.includes(INIT_CALL)) {
      contents = contents.replace(
        /(override fun onCreate\(\) \{\n\s+super\.onCreate\(\))/,
        `$1\n${INIT_CALL}`
      );
    }

    mod.modResults.contents = contents;
    return mod;
  });
};

export default createRunOncePlugin(withNativeLogger, 'withNativeLogger', '1.0.0');
