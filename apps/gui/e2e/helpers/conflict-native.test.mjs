import { expect, it } from 'vitest'
import { assertInteractiveConsole } from './conflict-native.mjs'

it('rejects locked or unknown console state before native input', () => {
  expect(() => assertInteractiveConsole([])).toThrow('unlocked macOS')
  expect(() => assertInteractiveConsole([{ kCGSSessionOnConsoleKey: true }])).toThrow()
  expect(() =>
    assertInteractiveConsole([
      {
        kCGSSessionOnConsoleKey: true,
        kCGSessionLoginDoneKey: true,
        CGSSessionScreenIsLocked: true,
      },
    ])
  ).toThrow()
})

it('allows a confirmed unlocked active session', () => {
  expect(() =>
    assertInteractiveConsole([
      {
        kCGSSessionOnConsoleKey: true,
        kCGSessionLoginDoneKey: true,
        CGSSessionScreenIsLocked: false,
      },
    ])
  ).not.toThrow()
})

it('allows the logged-in console when macOS omits the unlocked flag', () => {
  expect(() =>
    assertInteractiveConsole([{ kCGSSessionOnConsoleKey: true, kCGSessionLoginDoneKey: true }])
  ).not.toThrow()
})

it.each([false, undefined])('rejects an unfinished login (%s)', loginDone => {
  expect(() =>
    assertInteractiveConsole([
      {
        kCGSSessionOnConsoleKey: true,
        kCGSessionLoginDoneKey: loginDone,
        CGSSessionScreenIsLocked: false,
      },
    ])
  ).toThrow()
})

it.each([true, 'false', null, 0])('rejects locked or malformed lock flags (%s)', locked => {
  expect(() =>
    assertInteractiveConsole([
      {
        kCGSSessionOnConsoleKey: true,
        kCGSessionLoginDoneKey: true,
        CGSSessionScreenIsLocked: locked,
      },
    ])
  ).toThrow()
})
