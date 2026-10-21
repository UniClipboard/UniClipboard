import { act, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { updateSettings, getSettings, type Settings, type SettingsPatchInput } from '@/api/daemon'
import LocalDeviceListItem from '@/components/device/LocalDeviceListItem'
import LocalDevicePanel from '@/components/device/LocalDevicePanel'
import { SettingProvider } from '@/contexts/SettingContext'
import i18n from '@/i18n'
import { resetSettingsSyncForTest } from '@/lib/settings-sync'
import { makeBaseSettings } from '@/test/fixtures/settings'

const commits = vi.hoisted(() => ({ sync: 0, file: 0, info: 0, status: 0, list: 0 }))
vi.mock('@/api/daemon', () => ({ getSettings: vi.fn(), updateSettings: vi.fn() }))
vi.mock('@/lib/daemon-ws-bootstrap', () => ({
  connectDaemonWs: vi.fn().mockResolvedValue(undefined),
}))
vi.mock('@/lib/daemon-ws', () => import('@/test/fixtures/settings-ws'))
vi.mock('@/lib/ipc', () => ({
  commands: { setTrayLanguage: vi.fn().mockResolvedValue(undefined) },
}))
vi.mock('@/host/app', () => ({ getVersion: vi.fn().mockResolvedValue('1.0.0') }))

// Count commits below each real subscription boundary, without adding memoization in the probes.
vi.mock('@/components/ui/switch', async () => {
  const { useLayoutEffect } = await import('react')
  return {
    Switch: ({
      checked,
      disabled,
      onCheckedChange,
      'aria-label': label,
    }: {
      checked: boolean
      disabled: boolean
      onCheckedChange: (value: boolean) => void
      'aria-label': string
    }) => {
      useLayoutEffect(() => {
        commits[label === 'Enable sync' ? 'sync' : 'file'] += 1
      })
      return (
        <button
          type="button"
          role="switch"
          aria-label={label}
          aria-checked={checked}
          disabled={disabled}
          onClick={() => onCheckedChange(!checked)}
        />
      )
    },
  }
})
vi.mock('@/components/device/PanelFactRow', async () => {
  const { useLayoutEffect } = await import('react')
  return {
    default: function FactRowProbe({ children }: { children: React.ReactNode }) {
      useLayoutEffect(() => {
        commits.info += 1
      })
      return <div>{children}</div>
    },
  }
})
vi.mock('@/components/device/StatusDot', async () => {
  const { useLayoutEffect } = await import('react')
  return {
    default: function StatusProbe() {
      useLayoutEffect(() => {
        commits.status += 1
      })
      return null
    },
  }
})
vi.mock('@/components/device/DeviceListItem', async () => {
  const { useLayoutEffect } = await import('react')
  return {
    default: function ListItemProbe({ status }: { status: { label: string } }) {
      useLayoutEffect(() => {
        commits.list += 1
      })
      return <span data-testid="local-status">{status.label}</span>
    },
  }
})

async function setup() {
  render(
    <SettingProvider>
      <LocalDeviceListItem name="Mac" selected onSelect={() => {}} />
      <LocalDevicePanel localDevice={{ deviceName: 'Mac', peerId: 'local' }} memberCount={2} />
    </SettingProvider>
  )
  await waitFor(() =>
    expect(
      screen.getByRole('switch', { name: i18n.t('devices.panel.policies.syncEnabled.title') })
    ).toBeEnabled()
  )
  await waitFor(() => expect(screen.getByText('v1.0.0')).toBeInTheDocument())
  Object.assign(commits, { sync: 0, file: 0, info: 0, status: 0, list: 0 })
}

// The daemon stores each saved patch; the window reads it back after the save.
let stored: Settings
function store(patch: SettingsPatchInput) {
  stored = {
    ...stored,
    sync: { ...stored.sync, ...patch.sync },
    fileSync: { ...stored.fileSync, ...patch.fileSync },
  }
}

describe('local device render isolation', () => {
  beforeEach(async () => {
    resetSettingsSyncForTest()
    await i18n.changeLanguage('en-US')
    stored = makeBaseSettings()
    vi.mocked(getSettings).mockImplementation(async () => stored)
    vi.mocked(updateSettings).mockImplementation(async patch => {
      store(patch)
      return { success: true, restartRequired: false }
    })
  })

  it('updates only the file row while saving and completing a file-sync change', async () => {
    let finish!: () => void
    vi.mocked(updateSettings).mockImplementation(
      patch =>
        new Promise(resolve => {
          finish = () => {
            store(patch)
            resolve({ success: true, restartRequired: false })
          }
        })
    )
    await setup()
    const user = userEvent.setup()
    const file = screen.getByRole('switch', {
      name: i18n.t('devices.panel.policies.fileSync.title'),
    })
    await user.click(file)
    expect(commits).toMatchObject({ sync: 0, info: 0, status: 0, list: 0 })
    expect(commits.file).toBeGreaterThan(0)
    await act(async () => finish())
    await waitFor(() => expect(file).not.toBeChecked())
    expect(commits).toMatchObject({ sync: 0, info: 0, status: 0, list: 0 })
  })

  it('updates dependent controls and statuses for the master switch, without rerendering device information', async () => {
    await setup()
    await userEvent
      .setup()
      .click(
        screen.getByRole('switch', { name: i18n.t('devices.panel.policies.syncEnabled.title') })
      )
    await waitFor(() =>
      expect(screen.getByTestId('local-status')).toHaveTextContent(
        i18n.t('devices.thisDevice.syncPaused')
      )
    )
    expect(commits.info).toBe(0)
    expect(commits.sync).toBeGreaterThan(0)
    expect(commits.file).toBeGreaterThan(0)
    expect(commits.status).toBeGreaterThan(0)
    expect(commits.list).toBeGreaterThan(0)
  })
})
