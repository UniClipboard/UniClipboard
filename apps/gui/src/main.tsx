// Vite removes this preview branch from release builds.
if (import.meta.env.DEV && new URLSearchParams(window.location.search).has('upgrade-preview')) {
  void import('@/dev/upgrade-preview-entry')
} else {
  // Paint the startup screen from a small module first, then load the rest of the app. Loading
  // them in parallel would let the large module graph starve the small one.
  void import('@/startup-screen')
    .then(({ showStartupScreen }) => showStartupScreen())
    .catch(error => console.error('[main] startup screen failed:', error))
    .finally(() => void import('@/bootstrap'))
}
