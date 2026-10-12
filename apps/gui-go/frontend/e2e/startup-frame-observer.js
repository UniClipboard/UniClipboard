// Acceptance bundles only: observe painted entry frames without input or clipboard access.
;(() => {
  const frames = []
  window.__UC_STARTUP_FRAMES__ = frames
  let previous = ''
  let active = true
  window.__UC_STOP_STARTUP_FRAMES__ = () => {
    active = false
  }
  const sample = () => {
    if (!active) return
    const skin = document.querySelector('#root [data-ui-skin]')?.dataset.uiSkin
    if (document.querySelector('#root')?.children.length) {
      const phase = document.querySelector('.uc-splash') ? 'html' : 'react'
      const heading =
        document.querySelector('[data-testid="entry-kit"] h1, .uc-splash h1')?.textContent || ''
      const signature = JSON.stringify([skin || 'legacy', phase, heading])
      if (signature !== previous) {
        previous = signature
        frames.push({ at: performance.now(), skin: skin || 'legacy', phase, heading })
      }
    }
    requestAnimationFrame(sample)
  }
  requestAnimationFrame(sample)
})()
