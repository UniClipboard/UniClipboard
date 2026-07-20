require "fileutils"
require "xcodeproj"

output = ARGV.fetch(0)
FileUtils.rm_rf(output)
project = Xcodeproj::Project.new(output)
target = project.new_target(:application, "EngineProbe", :ios, "17.0")

source_group = project.main_group.new_group(
  "EngineProbe",
  "../../apps/ios-probe/EngineProbe"
)
%w[
  EngineProbeApp.swift
  ProbeBridge.swift
  ProbeModel.swift
  ProbeView.swift
].each do |name|
  target.source_build_phase.add_file_reference(source_group.new_file(name))
end
source_group.new_file("Info.plist")
source_group.new_file("EngineProbe-Bridging-Header.h")
source_group.new_file("uc_ios_probe.h")

target.build_configurations.each do |config|
  settings = config.build_settings
  settings["PRODUCT_BUNDLE_IDENTIFIER"] = "app.uniclipboard.EngineProbe"
  settings["DEVELOPMENT_TEAM"] = "8XG39X5CL8"
  settings["CODE_SIGN_STYLE"] = "Automatic"
  settings["SWIFT_VERSION"] = "5.0"
  settings["INFOPLIST_FILE"] = "../../apps/ios-probe/EngineProbe/Info.plist"
  settings["SWIFT_OBJC_BRIDGING_HEADER"] = "../../apps/ios-probe/EngineProbe/EngineProbe-Bridging-Header.h"
  settings["LIBRARY_SEARCH_PATHS"] = ["$(inherited)", "$(SRCROOT)/../../target/ios-probe-cargo/aarch64-apple-ios/release"]
  settings["OTHER_LDFLAGS"] = [
    "$(inherited)", "-luc_ios_probe_core", "-framework", "Security",
    "-framework", "SystemConfiguration", "-framework", "CoreFoundation",
    "-framework", "Network",
    "-lsqlite3", "-lz", "-liconv", "-lresolv",
  ]
  settings["TARGETED_DEVICE_FAMILY"] = "1"
  settings["SUPPORTED_PLATFORMS"] = "iphoneos"
  settings["SUPPORTS_MACCATALYST"] = "NO"
end

project.save
