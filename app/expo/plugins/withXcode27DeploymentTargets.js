"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.patchPodfileForXcode27 = void 0;
const config_plugins_1 = require("expo/config-plugins");
const GENERATED_TAG = 'uniclip-xcode-27-deployment-targets';
const POD_DEPLOYMENT_TARGET_PATCH = [
    "    minimum_deployment_target = Gem::Version.new(podfile_properties['ios.deploymentTarget'] || '16.4')",
    '    installer.pods_project.targets.each do |pod_target|',
    '      pod_target.build_configurations.each do |build_configuration|',
    "        current_deployment_target = build_configuration.build_settings['IPHONEOS_DEPLOYMENT_TARGET']&.to_s",
    '        next unless current_deployment_target&.match?(/\\A\\d+(?:\\.\\d+)*\\z/)',
    '',
    '        if Gem::Version.new(current_deployment_target) < minimum_deployment_target',
    "          build_configuration.build_settings['IPHONEOS_DEPLOYMENT_TARGET'] = minimum_deployment_target.to_s",
    '        end',
    '      end',
    '    end',
].join('\n');
const patchPodfileForXcode27 = (contents) => config_plugins_1.CodeGenerator.mergeContents({
    src: contents,
    newSrc: POD_DEPLOYMENT_TARGET_PATCH,
    tag: GENERATED_TAG,
    anchor: /post_install do \|installer\|/,
    offset: 1,
    comment: '#',
}).contents;
exports.patchPodfileForXcode27 = patchPodfileForXcode27;
const withXcode27DeploymentTargets = (config) => (0, config_plugins_1.withPodfile)(config, (config) => {
    config.modResults.contents = (0, exports.patchPodfileForXcode27)(config.modResults.contents);
    return config;
});
exports.default = (0, config_plugins_1.createRunOncePlugin)(withXcode27DeploymentTargets, 'withXcode27DeploymentTargets', '1.0.0');
