const { execFileSync } = require("node:child_process");
const { resolve } = require("node:path");

module.exports = ({ config }) => {
  const variant = process.env.UNIC_MOBILE_VARIANT ?? "test";
  if (variant === "test") return config;
  if (variant !== "production") throw new Error("Invalid UNIC_MOBILE_VARIANT");

  const { android, ios } = JSON.parse(execFileSync("python3", [
    resolve(__dirname, "../../tools/release/mobile.py"), "config",
  ], { encoding: "utf8" }));
  const platform = process.env.UNIC_MOBILE_RELEASE_PLATFORM;
  if (platform !== "ios" && platform !== "android") {
    throw new Error("Production configuration requires UNIC_MOBILE_RELEASE_PLATFORM=ios or android");
  }

  return {
    ...config,
    name: "UniClipboard",
    scheme: "uniclipboard",
    version: platform === "ios" ? ios.version : android.version,
    ios: {
      ...config.ios,
      bundleIdentifier: ios.application_id,
      buildNumber: String(ios.build_number),
      entitlements: {
        ...config.ios.entitlements,
        "com.apple.security.application-groups": [`group.${ios.application_id}.incoming-share`],
        "keychain-access-groups": [`$(AppIdentifierPrefix)${ios.application_id}`],
      },
      infoPlist: {
        ...config.ios.infoPlist,
        NSLocalNetworkUsageDescription: "Discover your devices and sync clipboard content over your local network.",
        NSPhotoLibraryUsageDescription: "Choose an image to add to clipboard history.",
        NSFaceIDUsageDescription: "Unlock an existing backup when a compatible credential is available.",
      },
    },
    android: {
      ...config.android,
      package: android.application_id,
      versionCode: android.build_number,
    },
    plugins: [...config.plugins, "./plugins/withUnsignedAndroidRelease.js"],
  };
};
