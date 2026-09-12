import { fireEvent, render, screen } from '@testing-library/react'
import { beforeEach, it, expect, vi } from 'vitest'
import { DeviceTrustDialogHost } from '@/components/device/DeviceTrustDialogHost'
import type { DeviceTrustContextValue } from '@/contexts/device-trust-context'
const { state } = vi.hoisted(() => ({
  state: { current: null as unknown as DeviceTrustContextValue },
}))
vi.mock('@/hooks/useDeviceTrust', () => ({ useDeviceTrust: () => state.current }))
vi.mock('@/hooks/useDeviceTrustDesktopEffects', () => ({ useDeviceTrustDesktopEffects: () => {} }))

const groups = {
  revision: 1,
  deviceTrust: {
    revision: 1,
    localDeviceId: 'local',
    localMembership: 'active' as const,
    currentChange: null,
    devices: [],
    allowedActions: [],
    recovery: 'not_available_in_this_version',
    updatedAtMs: 0,
    blockedReason: null,
  },
  issues: [
    {
      issueId: 'issue',
      choices: [
        {
          choiceId: 'choice',
          isCurrentGroup: true,
          requiresRePairing: false,
          memberDeviceIds: ['local'],
          membersComplete: true,
        },
      ],
    },
  ],
}
beforeEach(() => {
  state.current = {
    deviceGroups: groups,
    snapshot: groups.deviceTrust,
    loading: false,
    decisionBusy: false,
    decisionError: null,
    localRemovalConfirmationIssueId: null,
    localRemovalConfirmationChoiceId: null,
    decision: null,
    acknowledgeDecision: vi.fn(),
    cancelLocalConfirmation: vi.fn(),
    refresh: vi.fn(),
    choose: vi.fn(),
  }
})

it('stays hidden throughout the initial check when no issues are found', () => {
  state.current = { ...state.current, deviceGroups: null, snapshot: null }
  const { rerender } = render(<DeviceTrustDialogHost />)
  expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
  state.current = { ...state.current, loading: true }
  rerender(<DeviceTrustDialogHost />)
  expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
  state.current = { ...state.current, loading: false, deviceGroups: { ...groups, issues: [] } }
  rerender(<DeviceTrustDialogHost />)
  expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
})

it('opens when the initial check finds an issue', () => {
  state.current = { ...state.current, deviceGroups: null, snapshot: null, loading: true }
  const { rerender } = render(<DeviceTrustDialogHost />)
  expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
  state.current = {
    ...state.current,
    deviceGroups: groups,
    snapshot: groups.deviceTrust,
    loading: false,
  }
  rerender(<DeviceTrustDialogHost />)
  expect(screen.getByRole('dialog')).toBeInTheDocument()
})

it('shows an initial check failure and keeps retry available until recovery', () => {
  state.current = {
    ...state.current,
    deviceGroups: null,
    snapshot: null,
    decisionError: 'runtime_unavailable',
  }
  const { rerender } = render(<DeviceTrustDialogHost />)
  expect(screen.getByTestId('device-trust-error')).toBeVisible()
  fireEvent.click(screen.getByTestId('device-trust-recheck'))
  expect(state.current.refresh).toHaveBeenCalledOnce()
  state.current = { ...state.current, loading: true }
  rerender(<DeviceTrustDialogHost />)
  expect(screen.getByRole('dialog')).toBeInTheDocument()
  expect(screen.getByTestId('device-trust-recheck')).toBeDisabled()
  state.current = {
    ...state.current,
    loading: false,
    decisionError: null,
    deviceGroups: { ...groups, issues: [] },
  }
  rerender(<DeviceTrustDialogHost />)
  expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
})

it('keeps the same dialog through selection, submission, and completion', () => {
  const { rerender } = render(<DeviceTrustDialogHost />)
  const dialog = screen.getByRole('dialog')
  state.current = {
    ...state.current,
    decisionBusy: true,
    decision: { groups, issueId: 'issue', choiceId: 'choice', outcome: 'submitting' },
  }
  rerender(<DeviceTrustDialogHost />)
  expect(screen.getByRole('dialog')).toBe(dialog)
  state.current = {
    ...state.current,
    decisionBusy: false,
    deviceGroups: { ...groups, issues: [] },
    decision: { groups, issueId: 'issue', choiceId: 'choice', outcome: 'completed' },
  }
  rerender(<DeviceTrustDialogHost />)
  expect(screen.getByRole('dialog')).toBe(dialog)
  expect(screen.getByTestId('device-trust-done')).toBeEnabled()
  state.current = {
    ...state.current,
    decision: null,
    decisionError: 'runtime_unavailable',
  }
  rerender(<DeviceTrustDialogHost />)
  expect(screen.getByTestId('device-trust-error')).toBeVisible()
  fireEvent.click(screen.getByTestId('device-trust-recheck'))
  expect(state.current.refresh).toHaveBeenCalledOnce()
  state.current = { ...state.current, decisionError: null }
  rerender(<DeviceTrustDialogHost />)
  expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
})
