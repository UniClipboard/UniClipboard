Pod::Spec.new do |s|
  s.name           = 'UnicEngine'
  s.version        = '0.1.0'
  s.summary        = 'In-process Go Engine native Expo module for UniClipboard'
  s.description    = 'Bridges UnicEngine.xcframework (Go engine, tailcat, classify, text, v1migration) to Expo Modules.'
  s.license        = 'Proprietary'
  s.author         = 'UniClipboard'
  s.homepage       = 'https://github.com/UniClipboard/unic'
  s.platforms      = { :ios => '16.0' }
  s.swift_version  = '5.9'
  s.source         = { :git => '' }
  s.static_framework = true

  s.dependency 'ExpoModulesCore'
  s.vendored_frameworks = 'Frameworks/UnicCore.xcframework'
  s.source_files   = '*.{h,m,swift}'
  s.frameworks     = 'AVFoundation', 'MetricKit', 'Network', 'Security', 'UIKit', 'UniformTypeIdentifiers', 'VisionKit'
  s.libraries      = 'resolv'

  s.pod_target_xcconfig = {
    'DEFINES_MODULE' => 'YES'
  }
end
