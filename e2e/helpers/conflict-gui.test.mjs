import { afterEach, describe, expect, it } from 'vitest'
import { click, confirmSelection, recheckDecision } from './conflict-gui.js'

afterEach(() => document.body.replaceChildren())

it('does not confuse a previous review warning with a new submission', async () => {
  document.body.innerHTML =
    '<button data-testid="device-trust-choice-choice" aria-checked="true"></button><button data-testid="device-trust-confirm"></button><p data-error="device_state_changed"></p>'
  let clicks = 0
  const instance = {
    async execute(fn, arg) {
      return fn(arg)
    },
    async $() {
      return {
        async click() {
          clicks++
          document.body.innerHTML = '<button data-testid="device-trust-recheck" disabled></button>'
        },
      }
    },
    async waitUntil(condition) {
      expect(await condition()).toBe(true)
    },
  }
  expect(await confirmSelection(instance, 'choice')).toBe(true)
  expect(clicks).toBe(1)
})

it('returns to review when a changed choice disables confirmation', async () => {
  let clicked = false
  const element = {
    async waitForExist() {},
    async waitForDisplayed() {},
    async waitForEnabled() {
      throw new Error('selection cleared; confirmation stays disabled')
    },
    async click() {
      clicked = true
    },
  }
  const instance = {
    async $() {
      return element
    },
    async execute() {
      return 'review'
    },
    async waitUntil(condition) {
      expect(await condition()).toBe(true)
    },
  }
  expect(await confirmSelection(instance, 'choice')).toBe(false)
  expect(clicked).toBe(false)
})

it.each([
  { states: ['waiting', 'review'], submitted: false, clicks: 0 },
  { states: ['ready', 'review'], submitted: false, clicks: 0 },
  { states: ['ready', 'ready', 'review'], submitted: false, clicks: 1 },
  { states: ['waiting', 'ready', 'ready', 'submitted'], submitted: true, clicks: 1 },
])('keeps one submission attempt bounded across $states', async sample => {
  const states = [...sample.states]
  let clicks = 0
  const instance = {
    async execute() {
      return states.shift()
    },
    async $() {
      return {
        async click() {
          clicks++
        },
      }
    },
    async waitUntil(condition) {
      for (let attempt = 0; attempt < 10; attempt++) if (await condition()) return
      throw new Error('confirmation did not settle')
    },
  }
  expect(await confirmSelection(instance, 'choice')).toBe(sample.submitted)
  expect(clicks).toBe(sample.clicks)
})

it('waits for the driver element after the page replaces a button', async () => {
  let exists = false
  let clicked = 0
  const element = {
    async waitForExist() {
      exists = true
    },
    async waitForDisplayed() {
      expect(exists).toBe(true)
    },
    async waitForEnabled() {
      expect(exists).toBe(true)
    },
    async click() {
      if (!exists) throw new Error('elementId is missing after replacement')
      clicked++
    },
  }
  const instance = {
    async waitUntil(condition) {
      expect(await condition()).toBe(true)
    },
    async execute() {
      return true
    },
    async $() {
      return element
    },
  }
  await click(instance, '[data-testid="device-trust-done"]')
  expect(clicked).toBe(1)
})

function instanceAt(initial, completeOnClick = false) {
  let state = initial
  let clicks = 0
  return {
    get clicks() {
      return clicks
    },
    async waitUntil(condition) {
      expect(await condition()).toBe(true)
    },
    async $(selector) {
      const done = selector.includes('device-trust-done')
      return {
        isDisplayed: async () => done && state === 'done',
        isExisting: async () => (done ? state === 'done' : state === 'ready'),
        isEnabled: async () => !done && state === 'ready',
        async click() {
          clicks++
          state = 'done'
          if (completeOnClick) throw new Error('stale element')
        },
      }
    },
  }
}

describe('decision result recheck', () => {
  it('does not wait for a removed recheck button after completion', async () => {
    const instance = instanceAt('done')
    await recheckDecision(instance)
    expect(instance.clicks).toBe(0)
  })
  it('accepts completion that arrives between locating and clicking', async () => {
    const instance = instanceAt('ready', true)
    await recheckDecision(instance)
    expect(instance.clicks).toBe(1)
  })
  it('clicks recheck when the result is still pending', async () => {
    const instance = instanceAt('ready')
    await recheckDecision(instance)
    expect(instance.clicks).toBe(1)
  })
})
