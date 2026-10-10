import { describe, expect, it } from 'vitest'
import type { StartupStepDto } from '@/lib/daemon-startup-types'
import {
  isUpgradeRequired,
  pendingStartupSnapshot,
  startupPresentation,
} from '@/lib/startup-progress'
import type { StartupSnapshot } from '@/lib/startup-progress'

const step = (name: StartupStepDto) => ({
  step: name,
  processed: 0,
  total: null,
  unit: null,
  warning_count: null,
  completed: false,
})

const snapshot = (
  state: StartupSnapshot['state'],
  upgrade: Partial<NonNullable<StartupSnapshot['upgrade']>> | null
): StartupSnapshot => ({
  ...pendingStartupSnapshot,
  state,
  upgrade: upgrade && {
    required: true,
    recovering: false,
    completed: false,
    current_step: 'checking',
    steps: [step('checking')],
    ...upgrade,
  },
})

describe('isUpgradeRequired', () => {
  it('does not treat the daemon storage check as an upgrade', () => {
    expect(isUpgradeRequired(snapshot('starting_services', {}))).toBe(false)
    expect(isUpgradeRequired(snapshot('ready', {}))).toBe(false)
    expect(startupPresentation(snapshot('starting_services', {})).required).toBe(false)
  })

  it('treats active, recovering, failed or converting upgrades as real', () => {
    expect(isUpgradeRequired(snapshot('upgrading', {}))).toBe(true)
    expect(isUpgradeRequired(snapshot('starting_services', { recovering: true }))).toBe(true)
    expect(isUpgradeRequired(snapshot('failed', {}))).toBe(true)
    expect(
      isUpgradeRequired(snapshot('starting_services', { steps: [step('converting_contents')] }))
    ).toBe(true)
  })

  it('is false without upgrade info or when not required', () => {
    expect(isUpgradeRequired(snapshot('upgrading', null))).toBe(false)
    expect(isUpgradeRequired(snapshot('upgrading', { required: false }))).toBe(false)
  })
})
