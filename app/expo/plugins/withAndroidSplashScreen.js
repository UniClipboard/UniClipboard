const fs = require("node:fs");
const path = require("node:path");
const {
  AndroidConfig,
  withAndroidColors,
  withAndroidColorsNight,
  withAndroidStyles,
  withAppBuildGradle,
  withDangerousMod,
  withMainActivity,
} = require("expo/config-plugins");

const CORE_SPLASHSCREEN_DEPENDENCY = 'implementation("androidx.core:core-splashscreen:1.2.0")';
const LIGHT_BACKGROUND = "#FEF7FF";
const DARK_BACKGROUND = "#141218";
const SPLASH_CONTROLLER_IMPORT = "import app.uniclipboard.engine.AndroidSplashController";
const SPLASH_INSTALL_CALL = "    AndroidSplashController.install(this)";

const LEGACY_DENSITY_DIRS = [
  "drawable-mdpi",
  "drawable-hdpi",
  "drawable-xhdpi",
  "drawable-xxhdpi",
  "drawable-xxxhdpi",
  "drawable-night-mdpi",
  "drawable-night-hdpi",
  "drawable-night-xhdpi",
  "drawable-night-xxhdpi",
  "drawable-night-xxxhdpi",
];

function setAppThemeWindowBackground(styles) {
  return AndroidConfig.Styles.assignStylesValue(styles, {
    add: true,
    parent: { name: "AppTheme", parent: "Theme.AppCompat.DayNight.NoActionBar" },
    name: "android:windowBackground",
    value: "@color/splashscreen_background",
  });
}

function setSplashTheme(styles) {
  const resources = styles.resources ?? (styles.resources = {});
  const list = Array.isArray(resources.style) ? resources.style : [];
  const filtered = list.filter((entry) => entry?.$?.name !== "Theme.App.SplashScreen");
  filtered.push({
    $: {
      name: "Theme.App.SplashScreen",
      parent: "Theme.SplashScreen",
    },
    item: [
      { $: { name: "windowSplashScreenBackground" }, _: "@color/splashscreen_background" },
      { $: { name: "windowSplashScreenAnimatedIcon" }, _: "@drawable/splashscreen_logo" },
      { $: { name: "postSplashScreenTheme" }, _: "@style/AppTheme" },
    ],
  });
  resources.style = filtered;
  return styles;
}

function patchMainActivity(contents) {
  let next = contents;
  if (!next.includes(SPLASH_CONTROLLER_IMPORT)) {
    const pkgMatch = next.match(/^package\s+[^\n]+\n/m);
    if (!pkgMatch) {
      throw new Error("Expected package declaration in Android MainActivity.kt");
    }
    const insertAt = pkgMatch.index + pkgMatch[0].length;
    next = `${next.slice(0, insertAt)}\n${SPLASH_CONTROLLER_IMPORT}\n${next.slice(insertAt)}`;
  }

  // Let Theme.SplashScreen + installSplashScreen() apply postSplashScreenTheme.
  next = next.replace(/^([ \t]*)setTheme\(R\.style\.AppTheme\);?[ \t]*$/m, "$1// setTheme(R.style.AppTheme)");

  if (!next.includes("AndroidSplashController.install(this)")) {
    const anchor = /^([ \t]*)super\.onCreate\((?:null|savedInstanceState)\)[ \t]*$/m;
    if (!anchor.test(next)) {
      throw new Error("Expected super.onCreate(...) in Android MainActivity.kt");
    }
    next = next.replace(anchor, `${SPLASH_INSTALL_CALL}\n$&`);
  }

  return next;
}

/**
 * Configures an Android-only Material 3 launch splash screen backed by
 * androidx.core:core-splashscreen (light #FEF7FF + dark #141218) without
 * modifying iOS splash assets, storyboards, or native dependencies.
 */
module.exports = (config, options = {}) => {
  const lightBg = options.backgroundColor || LIGHT_BACKGROUND;
  const darkBg = options.darkBackgroundColor || DARK_BACKGROUND;
  const lightDrawableRel = options.drawable || "./assets/android-splash/splashscreen_logo.xml";
  const darkDrawableRel = options.darkDrawable || "./assets/android-splash/splashscreen_logo_dark.xml";

  config = withAndroidColors(config, (mod) => {
    mod.modResults = AndroidConfig.Colors.assignColorValue(mod.modResults, {
      name: "splashscreen_background",
      value: lightBg,
    });
    return mod;
  });

  config = withAndroidColorsNight(config, (mod) => {
    mod.modResults = AndroidConfig.Colors.assignColorValue(mod.modResults, {
      name: "splashscreen_background",
      value: darkBg,
    });
    return mod;
  });

  config = withAndroidStyles(config, (mod) => {
    mod.modResults = setAppThemeWindowBackground(mod.modResults);
    mod.modResults = setSplashTheme(mod.modResults);
    return mod;
  });

  config = withAppBuildGradle(config, (mod) => {
    if (mod.modResults.language !== "groovy") {
      throw new Error("withAndroidSplashScreen requires a Groovy app/build.gradle");
    }
    if (!mod.modResults.contents.includes("androidx.core:core-splashscreen")) {
      const depsBlock = /(\bdependencies\s*\{)/;
      if (!depsBlock.test(mod.modResults.contents)) {
        throw new Error("Expected dependencies block in Android app/build.gradle");
      }
      mod.modResults.contents = mod.modResults.contents.replace(
        depsBlock,
        `$1\n    ${CORE_SPLASHSCREEN_DEPENDENCY}`,
      );
    }
    return mod;
  });

  config = withMainActivity(config, (mod) => {
    if (mod.modResults.language !== "kt") {
      throw new Error("withAndroidSplashScreen requires a Kotlin MainActivity.kt");
    }
    mod.modResults.contents = patchMainActivity(mod.modResults.contents);
    return mod;
  });

  config = withDangerousMod(config, [
    "android",
    async (mod) => {
      const projectRoot = mod.modRequest.projectRoot;
      const resDir = path.join(projectRoot, "android/app/src/main/res");

      // Remove Expo's default placeholder raster splash logos so our vector
      // drawable resolves cleanly across all screen densities.
      for (const densityDir of LEGACY_DENSITY_DIRS) {
        const legacyPng = path.join(resDir, densityDir, "splashscreen_logo.png");
        await fs.promises.rm(legacyPng, { force: true });
      }

      const lightSrc = path.resolve(projectRoot, lightDrawableRel);
      const darkSrc = path.resolve(projectRoot, darkDrawableRel);
      const lightDst = path.join(resDir, "drawable", "splashscreen_logo.xml");
      const darkDst = path.join(resDir, "drawable-night", "splashscreen_logo.xml");

      await fs.promises.mkdir(path.dirname(lightDst), { recursive: true });
      await fs.promises.mkdir(path.dirname(darkDst), { recursive: true });
      await fs.promises.copyFile(lightSrc, lightDst);
      await fs.promises.copyFile(darkSrc, darkDst);

      return mod;
    },
  ]);

  return config;
};
