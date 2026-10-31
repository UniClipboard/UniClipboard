export const suites = {
  'settings-sidebar': { fixture: 'settings-page', platforms: ['macos', 'linux', 'windows'] },
  'settings-sidebar-motion': {
    fixture: 'settings-page',
    platforms: ['macos', 'linux'],
    viewport: { width: 560, height: 800 },
    reducedMotion: 'no-preference',
  },
  appearance: {
    fixture: 'appearance',
    platforms: ['none'],
    viewport: { width: 1100, height: 800 },
  },
  'app-entry': { fixture: 'app-entry', platforms: ['none'] },
  'app-entry-kit': { fixture: 'app-entry-kit', platforms: ['macos', 'linux', 'windows'] },
  'history-row-hover': { fixture: 'typography', platforms: ['none'] },
  typography: { fixture: 'typography', platforms: ['none'] },
  'mobile-connection': { fixture: 'mobile-connection', platforms: ['none'] },
  'theme-palette': {
    fixture: 'theme-palette',
    platforms: ['macos', 'linux', 'windows'],
    viewport: { width: 1000, height: 800 },
  },
  'ui-kit': {
    fixture: 'ui-kit-gallery',
    platforms: ['macos', 'linux', 'windows'],
    viewport: { width: 1720, height: 1200 },
  },
  'visual-effects': {
    fixture: 'visual-effects',
    platforms: ['macos', 'linux', 'windows'],
    reducedMotion: 'no-preference',
    viewport: { width: 1100, height: 800 },
  },
  'file-preview': {
    fixture: 'file-preview',
    platforms: ['macos', 'linux', 'windows'],
    viewport: { width: 650, height: 760 },
  },
  'history-detail-regressions': {
    fixture: 'history-detail-regressions',
    platforms: ['macos', 'linux', 'windows'],
    viewport: { width: 850, height: 750 },
  },
  'history-compact': {
    fixture: 'history-compact',
    platforms: ['linux'],
    viewport: { width: 739, height: 945 },
  },
  'device-tasks': {
    fixture: 'device-tasks',
    platforms: ['macos', 'linux', 'windows'],
    viewport: { width: 1000, height: 800 },
  },
  'settings-design': {
    fixture: 'settings',
    platforms: ['none'],
    viewport: { width: 1100, height: 800 },
  },
  'settings-scroll-memory': { fixture: 'settings-page', platforms: ['none'] },
  'history-tag-menu': {
    fixture: 'history-tag-menu',
    platforms: ['macos'],
    viewport: { width: 1280, height: 800 },
  },
  'startup-first-frame': {
    fixture: 'main-document',
    platforms: ['macos', 'windows', 'linux', 'none'],
  },
  'sidebar-animation': {
    fixture: 'sidebar-animation',
    platforms: ['macos'],
    reducedMotion: 'no-preference',
  },
  'macos-design': { fixture: 'macos-design-preview.html', platforms: ['macos'] },
}
export const selectedSuite = process.env.UC_TESTER_SUITE || 'settings-sidebar'
export const selectedPlatform = process.env.UC_TESTER_PLATFORM || 'macos'
export const selected = suites[selectedSuite]
if (!selected || !selected.platforms.includes(selectedPlatform)) {
  throw new Error(`Unsupported suite/platform: ${selectedSuite}/${selectedPlatform}`)
}

// Only independently passing suite/skin combinations enter the normal CI run.
export const verifiedPlatforms = {
  'startup-first-frame': ['macos', 'windows', 'linux', 'none'],
  'sidebar-animation': ['macos'],
  'settings-sidebar': ['macos', 'linux', 'windows'],
  'settings-sidebar-motion': ['macos', 'linux'],
  appearance: ['none'],
  'app-entry': ['none'],
  'app-entry-kit': ['macos', 'linux', 'windows'],
  'mobile-connection': ['none'],
  'theme-palette': ['macos', 'linux', 'windows'],
  'ui-kit': ['macos', 'linux', 'windows'],
  'visual-effects': ['macos', 'linux', 'windows'],
  'file-preview': ['macos', 'linux', 'windows'],
  'device-tasks': ['macos', 'linux', 'windows'],
  'settings-scroll-memory': ['none'],
  'history-tag-menu': ['macos'],
  'history-compact': ['linux'],
  'macos-design': ['macos'],
}
export const retainedReasons = {
  'history-row-hover': 'original history-favorite DOM lookup is null in the legacy runner too',
  typography: 'original transfer-progress 50% assertion times out in the legacy runner too',
  'history-detail-regressions':
    'original warm-image-stable assertion fails in the legacy runner too',
  'settings-design': 'legacy startup-mode combobox assertion no longer matches the current control',
}
