const { AndroidConfig, withAndroidManifest } = require("expo/config-plugins");

module.exports = (config) => withAndroidManifest(config, (mod) => {
  const activity = AndroidConfig.Manifest.getMainActivityOrThrow(mod.modResults);
  const filters = activity["intent-filter"] ??= [];
  for (const action of ["android.intent.action.SEND", "android.intent.action.SEND_MULTIPLE"]) {
    if (filters.some((filter) => filter.action?.some((entry) => entry.$["android:name"] === action))) continue;
    filters.push({
      action: [{ $: { "android:name": action } }],
      category: [{ $: { "android:name": "android.intent.category.DEFAULT" } }],
      data: [{ $: { "android:mimeType": "*/*" } }],
    });
  }
  return mod;
});
