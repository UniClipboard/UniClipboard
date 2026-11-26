module.exports = (config) => ({
  type: "share",
  name: "UniClipboardShare",
  displayName: "Unic",
  bundleIdentifier: ".Share",
  deploymentTarget: "17.0",
  exportJs: false,
  entitlements: {
    "com.apple.security.application-groups": (
      config.ios.entitlements["com.apple.security.application-groups"] ?? []
    ).filter((group) => group.endsWith(".incoming-share")),
  },
});
