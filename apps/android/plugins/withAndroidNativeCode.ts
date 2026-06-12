import { ConfigPlugin, withAppBuildGradle, createRunOncePlugin } from 'expo/config-plugins';

/**
 * Configures gradle to include native Android code from src/android directory
 * Adds sourceSets configuration to build.gradle to compile in-place
 */
const withAndroidNativeCode: ConfigPlugin = (config) => {
  // Modify build.gradle to add sourceSets configuration
  config = withAppBuildGradle(config, (config) => {
    const srcAndroidPath = '../../src/android';

    // Build the sourceSets configuration with proper Gradle syntax
    const sourceSetConfig = `

    // Native code source sets
    sourceSets {
      main {
        java.srcDirs += '${srcAndroidPath}/app/src/main/java'
        res.srcDirs += '${srcAndroidPath}/app/src/main/res'
        assets.srcDirs += '${srcAndroidPath}/app/src/main/assets'
      }
    }
`;

    // Check if sourceSets already exists
    if (!config.modResults.contents.includes('sourceSets {')) {
      // Find the closing brace of android block and insert before it
      const androidBlockMatch = config.modResults.contents.match(/^android\s*\{[\s\S]*?\n\}/m);

      if (androidBlockMatch) {
        const androidBlock = androidBlockMatch[0];
        const modifiedBlock = androidBlock.replace(/\n\}$/, sourceSetConfig + '\n}');
        config.modResults.contents = config.modResults.contents.replace(
          androidBlock,
          modifiedBlock
        );
        console.log('✓ Added sourceSets configuration to build.gradle');
      }
    } else {
      console.log('ℹ sourceSets already configured in build.gradle');
    }

    return config;
  });

  return config;
};

export default createRunOncePlugin(withAndroidNativeCode, 'withAndroidNativeCode', '1.0.0');
