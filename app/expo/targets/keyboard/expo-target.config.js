module.exports = (config) => ({
  type: "keyboard",
  name: "UniClipboardKeyboard",
  displayName: "Unic",
  bundleIdentifier: ".Keyboard",
  deploymentTarget: "17.0",
  exportJs: false,
  entitlements: {
    "com.apple.security.application-groups": config.ios.entitlements["com.apple.security.application-groups"],
  },
});
