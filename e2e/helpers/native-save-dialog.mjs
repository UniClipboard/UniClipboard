import { spawnSync } from 'node:child_process'

// Drives the real macOS save panel of the app under test through System Events. The controlling
// terminal needs the Accessibility permission; without it the dialog stays open and callers time out.
export function confirmNativeSaveDialog({ processName, directory }) {
  if (process.platform !== 'darwin') throw new Error('The native save dialog helper is macOS only')
  const script = `
    tell application "System Events"
      set deadline to (current date) + 20
      repeat until (exists sheet 1 of window 1 of process "${processName}") or ((current date) > deadline)
        delay 0.3
      end repeat
      tell process "${processName}"
        set frontmost to true
        keystroke "g" using {command down, shift down}
        delay 0.8
        keystroke "${directory.replaceAll('\\', '\\\\').replaceAll('"', '\\"')}"
        delay 0.5
        key code 36
        delay 0.8
        key code 36
      end tell
    end tell`
  const result = spawnSync('osascript', ['-e', script], { encoding: 'utf8' })
  if (result.status !== 0) throw new Error(`Save dialog automation failed: ${result.stderr}`)
}
