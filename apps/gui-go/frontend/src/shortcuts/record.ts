import { normalizeHotkey } from '@/shortcuts/normalize'

/** The fields of a keydown event the recorder reads. */
export type RecordedKeyEvent = Pick<
  KeyboardEvent,
  'key' | 'code' | 'ctrlKey' | 'altKey' | 'shiftKey' | 'metaKey'
>

/** Physical modifier keys, which alone never form a chord segment. */
const MODIFIER_KEYS = new Set(['Control', 'Shift', 'Alt', 'Meta'])

/**
 * Names of the physical keys other than letters and digits, by `KeyboardEvent.code`. Every name is
 * in the stored token set (ADR-013 section 8.1.1), so a recorded value is never refused for its
 * form; the code names also match the in-app handlers, which compare codes. A key without an entry
 * (a key only some layouts have, F25 and above) cannot be recorded (ADR-013 section 10).
 */
const CODE_KEY_NAMES: Record<string, string> = {
  Space: 'space',
  Enter: 'enter',
  Tab: 'tab',
  Backspace: 'backspace',
  Delete: 'delete',
  Insert: 'insert',
  Home: 'home',
  End: 'end',
  PageUp: 'pageup',
  PageDown: 'pagedown',
  ArrowUp: 'arrowup',
  ArrowDown: 'arrowdown',
  ArrowLeft: 'arrowleft',
  ArrowRight: 'arrowright',
  Comma: 'comma',
  Period: 'period',
  Slash: 'slash',
  Semicolon: 'semicolon',
  Quote: 'quote',
  Backquote: 'backquote',
  BracketLeft: 'bracketleft',
  BracketRight: 'bracketright',
  Backslash: 'backslash',
  Equal: 'equal',
  Minus: 'minus',
  NumpadAdd: 'add',
  NumpadSubtract: 'subtract',
  NumpadMultiply: 'numpadmultiply',
  NumpadDivide: 'numpaddivide',
  NumpadDecimal: 'numpaddecimal',
  NumpadEnter: 'numpadenter',
  NumpadEqual: 'numpadequal',
  CapsLock: 'capslock',
  NumLock: 'numlock',
  ScrollLock: 'scrolllock',
  PrintScreen: 'printscreen',
  Pause: 'pause',
  AudioVolumeUp: 'audiovolumeup',
  AudioVolumeDown: 'audiovolumedown',
  AudioVolumeMute: 'audiovolumemute',
  MediaPlayPause: 'mediaplaypause',
  MediaStop: 'mediastop',
  MediaTrackNext: 'mediatracknext',
  MediaTrackPrevious: 'mediatrackprevious',
}

/** The key name for a code, or `null` when the key has none in the stored token set. */
function keyName(e: RecordedKeyEvent): string | null {
  // Numpad digits first: with NumLock on they report the same `key` as the digit row ('1'), but
  // they are other physical keys (`numpad1`); with NumLock off the key is 'End' and the code the same.
  const numpadDigit = /^Numpad([0-9])$/.exec(e.code)
  if (numpadDigit) return `numpad${numpadDigit[1]}`
  // A Latin letter or digit is kept as the layout produces it (Dvorak, AZERTY); Option symbols,
  // dead keys and non-Latin letters fall back to the physical letter or digit.
  if (/^[a-z0-9]$/i.test(e.key)) return e.key.toLowerCase()
  const physicalKey = /^(?:Key([A-Z])|Digit([0-9]))$/.exec(e.code)
  if (physicalKey) return (physicalKey[1] ?? physicalKey[2]).toLowerCase()
  const functionKey = /^F([1-9]|1[0-9]|2[0-4])$/.exec(e.code)
  if (functionKey) return `f${functionKey[1]}`
  return CODE_KEY_NAMES[e.code] ?? null
}

export type RecordedCombo = { combo: string } | { unsupported: string }

/**
 * Build a normalized single combo from a keydown event, or `null` if the event
 * is a bare modifier press (we wait for a real key). Modifier state is read off
 * the event so holding Cmd and tapping V twice yields two `meta+v` segments.
 */
export function comboFromEvent(e: RecordedKeyEvent): RecordedCombo | null {
  if (MODIFIER_KEYS.has(e.key)) return null
  const key = keyName(e)
  if (!key) return { unsupported: e.key && e.key !== 'Unidentified' ? e.key : e.code || '?' }
  const parts: string[] = []
  if (e.ctrlKey) parts.push('ctrl')
  if (e.altKey) parts.push('alt')
  if (e.shiftKey) parts.push('shift')
  if (e.metaKey) parts.push('meta')
  parts.push(key)
  return { combo: normalizeHotkey(parts.join('+')) }
}
