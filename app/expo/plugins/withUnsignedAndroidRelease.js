const { withAppBuildGradle } = require("expo/config-plugins");

module.exports = (config) => withAppBuildGradle(config, (mod) => {
  const signing = /(\brelease\s*\{[\s\S]*?)^([ \t]*)signingConfig signingConfigs\.debug[ \t]*$/m;
  if (mod.modResults.language !== "groovy" || !signing.test(mod.modResults.contents)) {
    throw new Error("Expected the Expo Android release signing template");
  }
  mod.modResults.contents = mod.modResults.contents.replace(signing, "$1$2signingConfig null");
  return mod;
});
