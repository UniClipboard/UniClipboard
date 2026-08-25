import { configureStore } from '@reduxjs/toolkit'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { Provider } from 'react-redux'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import LocalDevicePanel from '@/components/device/LocalDevicePanel'
import spacesReducer from '@/store/spacesSlice'

vi.mock('@tauri-apps/api/app', () => ({
  getVersion: vi.fn().mockResolvedValue('1.0.0-test'),
}))

const updateSyncSetting = vi.hoisted(() => vi.fn().mockResolvedValue(undefined))
const updateFileSyncSetting = vi.hoisted(() => vi.fn().mockResolvedValue(undefined))

vi.mock('@/hooks/useSetting', () => ({
  useSetting: () => ({
    setting: {
      sync: { syncEnabled: true },
      fileSync: { fileSyncEnabled: true },
    },
    updateSyncSetting,
    updateFileSyncSetting,
  }),
}))

vi.mock('@/lib/platform', () => ({
  detectPlatformInfo: () => ({ isMac: false, isWindows: true, isLinux: false }),
}))

function renderPanel() {
  const store = configureStore({ reducer: { spaces: spacesReducer } })
  render(
    <Provider store={store}>
      <LocalDevicePanel
        localDevice={{ peerId: 'peer-windows', deviceName: 'Office PC' }}
        memberCount={2}
      />
    </Provider>
  )
}

describe('LocalDevicePanel space actions', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    document.elementFromPoint = vi.fn(() => null)
  })

  it('opens the non-destructive add-space dialog from the normal join entry', async () => {
    const user = userEvent.setup()
    renderPanel()

    await user.click(screen.getByRole('button', { name: /join (another )?space/i }))

    expect(screen.getByRole('heading', { name: 'Add a space' })).toBeInTheDocument()
    expect(screen.queryByText(/clipboard history will be re-encrypted/i)).not.toBeInTheDocument()
  })

  it('keeps the destructive switch dialog behind an explicit legacy recovery entry', async () => {
    const user = userEvent.setup()
    renderPanel()

    await user.click(screen.getByRole('button', { name: 'Recover legacy space' }))

    expect(screen.getByRole('heading', { name: 'Switch to another space' })).toBeInTheDocument()
  })
})
