import { fireEvent, render, screen } from '@testing-library/react'
import { it, expect, vi } from 'vitest'
import { DeviceTrustDialogHost } from '@/components/device/DeviceTrustDialogHost'
import type { DeviceTrustContextValue } from '@/contexts/device-trust-context'
const { state } = vi.hoisted(() => ({
  state: { current: null as unknown as DeviceTrustContextValue },
}))
vi.mock('@/hooks/useDeviceTrust', () => ({ useDeviceTrust: () => state.current }))
vi.mock('@/hooks/useDeviceTrustDesktopEffects', () => ({ useDeviceTrustDesktopEffects: () => {} }))

it('keeps the same dialog through selection, submission, and completion', () => {
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
