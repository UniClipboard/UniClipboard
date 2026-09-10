import { themePresets } from '../../src/lib/theme-engine'

// Export the existing theme authority for the Rust renderer; never maintain a second palette.
const output = JSON.stringify(themePresets, null, 2) + '\n'
const file = new URL('./assets/themes.json', import.meta.url)
if (process.argv.includes('--check')) {
  if ((await Bun.file(file).text()) !== output) throw new Error('Theme export is stale')
} else {
  await Bun.write(file, output)
}
