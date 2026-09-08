import { execFileSync } from 'node:child_process'

export function assertInteractiveConsole(sessions) {
  const active = sessions.find(session => session.kCGSSessionOnConsoleKey === true)
  // macOS omits the lock flag for an unlocked, logged-in console session.
  const lockFlag = active?.CGSSessionScreenIsLocked
  if (
    !active ||
    active.kCGSessionLoginDoneKey !== true ||
    (lockFlag !== undefined && lockFlag !== false)
  )
    throw new Error('Native keyboard test requires an unlocked macOS session; no keys were sent')
}

export function assertNativeKeyboardAvailable() {
  const plist = execFileSync('ioreg', ['-a', '-n', 'Root', '-d1'])
  const sessions = JSON.parse(
    execFileSync('plutil', ['-extract', 'IOConsoleUsers', 'json', '-o', '-', '-'], {
      input: plist,
      encoding: 'utf8',
    })
  )
  assertInteractiveConsole(sessions)
}
