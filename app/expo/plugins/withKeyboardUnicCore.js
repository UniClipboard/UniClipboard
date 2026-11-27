const { withMod, createRunOncePlugin } = require("expo/config-plugins");
const { PBXBuildFile, PBXFileReference, PBXNativeTarget } = require("@bacons/xcode");

const KEYBOARD_TARGET_NAME = "UniClipboardKeyboard";
const FRAMEWORK_REL_PATH = "../modules/unic-engine/ios/Frameworks/UnicCore.xcframework";

function appendSetting(current, additions) {
  const list = Array.isArray(current)
    ? [...current]
    : typeof current === "string" && current.trim().length > 0
      ? current.split(/\s+/).filter(Boolean)
      : ["$(inherited)"];
  for (const item of additions) {
    if (!list.includes(item)) {
      list.push(item);
    }
  }
  return list;
}

function linkUnicCoreIntoKeyboard(project) {
  const targets = project.rootObject?.props?.targets ?? [];
  const keyboardTarget = targets.find(
    (t) =>
      PBXNativeTarget.is(t) &&
      (t.props.name === KEYBOARD_TARGET_NAME || t.props.productName === KEYBOARD_TARGET_NAME),
  );
  if (!keyboardTarget) {
    return project;
  }

  // Ensure PBXFileReference for UnicCore.xcframework (shared dynamic framework, NOT embedded in .appex).
  const mainGroup = project.rootObject.props.mainGroup;
  let frameworksGroup = mainGroup.props.children.find(
    (child) => child?.props?.name === "Frameworks",
  );
  if (!frameworksGroup) {
    frameworksGroup = mainGroup;
  }

  let fileRef = frameworksGroup.props.children.find(
    (child) => PBXFileReference.is(child) && child.props.path === FRAMEWORK_REL_PATH,
  );
  if (!fileRef) {
    fileRef = PBXFileReference.create(project, {
      lastKnownFileType: "wrapper.xcframework",
      name: "UnicCore.xcframework",
      path: FRAMEWORK_REL_PATH,
      sourceTree: "<group>",
    });
    frameworksGroup.props.children.push(fileRef);
  }

  // Add to UniClipboardKeyboard's Frameworks build phase (Link Binary With Libraries only).
  const frameworksPhase = keyboardTarget.getFrameworksBuildPhase();
  if (!frameworksPhase.getBuildFile(fileRef)) {
    const buildFile = PBXBuildFile.create(project, {
      fileRef,
    });
    frameworksPhase.props.files.push(buildFile);
  }

  // Ensure @executable_path/../../Frameworks rpath and framework search path on all configs.
  const configs = keyboardTarget.props.buildConfigurationList?.props?.buildConfigurations ?? [];
  for (const cfg of configs) {
    const bs = cfg.props.buildSettings;
    bs.LD_RUNPATH_SEARCH_PATHS = appendSetting(bs.LD_RUNPATH_SEARCH_PATHS, [
      "$(inherited)",
      "@executable_path/Frameworks",
      "@executable_path/../../Frameworks",
    ]);
    bs.FRAMEWORK_SEARCH_PATHS = appendSetting(bs.FRAMEWORK_SEARCH_PATHS, [
      "$(inherited)",
      "$(SRCROOT)/../modules/unic-engine/ios/Frameworks",
    ]);
    bs.APPLICATION_EXTENSION_API_ONLY = "YES";
  }

  return project;
}

const withKeyboardUnicCore = (config) =>
  withMod(config, {
    platform: "ios",
    mod: "xcodeProjectBeta2",
    action: (cfg) => {
      cfg.modResults = linkUnicCoreIntoKeyboard(cfg.modResults);
      return cfg;
    },
  });

module.exports = createRunOncePlugin(withKeyboardUnicCore, "withKeyboardUnicCore", "1.0.0");
