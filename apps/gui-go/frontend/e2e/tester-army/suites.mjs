import { suites } from './catalog.mjs'
export { suites, verifiedPlatforms, retainedReasons } from './catalog.mjs'

export const selectedSuite = process.env.UC_TESTER_SUITE || 'settings-sidebar'
export const selectedPlatform = process.env.UC_TESTER_PLATFORM || 'macos'
export const selected = suites[selectedSuite]
if (!selected || !selected.platforms.includes(selectedPlatform)) {
  throw new Error(`Unsupported suite/platform: ${selectedSuite}/${selectedPlatform}`)
}
