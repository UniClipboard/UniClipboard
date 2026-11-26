const { withAppBuildGradle } = require("expo/config-plugins");

const ABI_FILTER_MARKER = "// unic-optional-abi-filters";
const ABI_FILTER_SNIPPET = `        ${ABI_FILTER_MARKER}
        def unicAbiFilters = findProperty('unicAbiFilters')
        if (unicAbiFilters) {
            ndk {
                abiFilters(*unicAbiFilters.toString().split(','))
            }
        }`;

module.exports = (config) => withAppBuildGradle(config, (mod) => {
  const signing = /(\brelease\s*\{[\s\S]*?)^([ \t]*)signingConfig signingConfigs\.debug[ \t]*$/m;
  if (mod.modResults.language !== "groovy" || !signing.test(mod.modResults.contents)) {
    throw new Error("Expected the Expo Android release signing template");
  }
  mod.modResults.contents = mod.modResults.contents.replace(signing, "$1$2signingConfig null");
  if (!mod.modResults.contents.includes(ABI_FILTER_MARKER)) {
    const defaultConfig = /(\bdefaultConfig\s*\{)/;
    if (!defaultConfig.test(mod.modResults.contents)) {
      throw new Error("Expected Android defaultConfig block in build.gradle");
    }
    mod.modResults.contents = mod.modResults.contents.replace(
      defaultConfig,
      `$1\n${ABI_FILTER_SNIPPET}`,
    );
  }
  return mod;
});
