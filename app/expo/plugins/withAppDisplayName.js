const { AndroidConfig, withInfoPlist, withStringsXml } = require("expo/config-plugins");

/**
 * Sets the user-visible app display name on iOS (CFBundleDisplayName) and
 * Android (res/values/strings.xml app_name) without changing the internal
 * Expo/Xcode project name, bundle identifier, or Android package name.
 */
module.exports = (config, options = {}) => {
  const displayName = typeof options === "string" ? options : options.displayName;
  if (!displayName) {
    throw new Error("withAppDisplayName requires a non-empty displayName");
  }

  config = withInfoPlist(config, (mod) => {
    mod.modResults.CFBundleDisplayName = displayName;
    return mod;
  });

  config = withStringsXml(config, (mod) => {
    mod.modResults = AndroidConfig.Strings.setStringItem(
      [{ $: { name: "app_name", translatable: "false" }, _: displayName }],
      mod.modResults,
    );
    return mod;
  });

  return config;
};
