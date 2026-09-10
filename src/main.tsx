// Vite removes this preview branch from release builds.
if (import.meta.env.DEV && new URLSearchParams(window.location.search).has('upgrade-preview')) {
  void import('@/dev/upgrade-preview-entry')
} else {
  void import('@/bootstrap')
}
