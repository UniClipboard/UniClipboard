export const suites = {
  'quick-panel-accessibility': {
    fixture: 'settings-page',
    platforms: ['macos', 'windows', 'linux'],
  },
  'delivery-status': {
    fixture: 'delivery-status',
    platforms: ['macos', 'windows', 'linux'],
    viewport: { width: 686, height: 650 },
  },
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
  'receive-generation': { fixture: 'file-preview', platforms: ['macos'] },
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
// Only independently passing suite/skin combinations enter the normal CI run.
export const verifiedPlatforms = {
  'delivery-status': ['macos', 'windows', 'linux'],
  'quick-panel-accessibility': ['macos', 'windows', 'linux'],
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
  'receive-generation': ['macos'],
  'device-tasks': ['macos', 'linux', 'windows'],
  'settings-scroll-memory': ['none'],
  'history-tag-menu': ['macos'],
  'history-compact': ['linux'],
  'macos-design': ['macos'],
}
export const retainedReasons = {
  'history-row-hover': 'original history-favorite DOM lookup is null in the legacy runner too',
  typography:
    'current byte label and 50% fill pass; the original content/progress non-overlap assertion still fails at 360px',
  'history-detail-regressions':
    'original warm-image-stable assertion fails in the legacy runner too',
  'settings-design': 'legacy startup-mode combobox assertion no longer matches the current control',
}

export const backendSuites = {
  'history-window-grid': { platforms: ['macos', 'windows', 'linux'], seeds: ['history'] },
  'tag-manager-layout': { platforms: ['macos', 'linux'], seeds: ['empty'] },
  'history-kit': { platforms: ['macos', 'windows', 'linux'], seeds: ['history', 'empty'] },
  'devices-kit': { platforms: ['macos', 'windows', 'linux'], seeds: ['devices'] },
  'quick-panel-unlock': { platforms: ['linux', 'macos', 'windows'], seeds: ['panel'] },
  'sidebar-toggle-app': { platforms: ['macos'], seeds: ['empty'] },
  'history-search': { platforms: ['none'], seeds: ['search'] },
}
// Each entry has passed its stated functional contract; retained candidates stay explicit.
export const verifiedBackendRuns = [
  { suite: 'history-kit', platform: 'windows', seed: 'history' },
  ...['macos', 'windows', 'linux'].map(platform => ({
    suite: 'history-window-grid',
    platform,
    seed: 'history',
  })),
  { suite: 'sidebar-toggle-app', platform: 'macos', seed: 'empty' },
  ...['macos', 'linux'].map(platform => ({ suite: 'tag-manager-layout', platform, seed: 'empty' })),
  ...['macos', 'windows', 'linux'].map(platform => ({
    suite: 'quick-panel-unlock',
    platform,
    seed: 'panel',
  })),
  ...['macos', 'windows', 'linux'].map(platform => ({
    suite: 'devices-kit',
    platform,
    seed: 'devices',
  })),
  ...['macos', 'windows', 'linux'].map(platform => ({
    suite: 'history-kit',
    platform,
    seed: 'empty',
  })),
  { suite: 'history-search', platform: 'none', seed: 'search', phase: 'components' },
]
export const retainedBackendReasons = {
  'history-kit/history':
    'the full original suite retains image, material, rail and focus assertions; focused bounded-grid acceptance is separate',
  'history-search/fullPage':
    'original WebDriver and replacement both fail the old detail geometry predicate; legacy rail assumptions remain in the Windows/Linux branches',
}
