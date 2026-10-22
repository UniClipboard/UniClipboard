import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { I18nextProvider } from 'react-i18next'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '@/i18n'
import { DevicesKitSheet } from '@/pages/devices-kit/DevicesKitSheet'
import type { DevicesKitController } from '@/pages/devices-kit/useDevicesKit'
import type {
  ButtonProps,
  CodeFieldProps,
  FormFieldProps,
  InlineNoticeProps,
  MenuProps,
  SheetProps,
  TaskPanelProps,
} from '@/ui-kit/contract'

const api = vi.hoisted(() => ({ switchSpace: vi.fn(), cancelJoinSpace: vi.fn() }))
vi.mock('@/api/daemon/setupV2', () => ({
  ...api,
  SetupV2Error: class extends Error {},
}))
vi.mock('@/hooks/useJoinAdmission', () => ({ useJoinAdmission: vi.fn() }))
vi.mock('@/store/hooks', () => ({ useAppDispatch: () => vi.fn() }))
vi.mock('@/hooks/useShortcutLayer', () => ({ useShortcutLayer: vi.fn() }))
vi.mock('@/hooks/useDevicesPage', async () => {
  const { useState } = await import('react')
  return {
    useDevicesPage: () => {
      const [switchSpaceOpen, setSwitchSpaceOpen] = useState(false)
      const [addDialogOpen, setAddDialogOpen] = useState(false)
      const [settingsSheetOpen, setSettingsSheetOpen] = useState(false)
      return {
        switchSpaceOpen,
        setSwitchSpaceOpen,
        addDialogOpen,
        settingsSheetOpen,
        peers: [],
        mobileDevices: [],
        trustListView: { removedDevices: [] },
        mobileActions: {
          setSettingsSheetOpen,
          setSettings: vi.fn(),
          setAddDialogOpen,
          handleAddClick: () => setAddDialogOpen(true),
          openSettings: () => setSettingsSheetOpen(true),
          reload: vi.fn(),
        },
        revokeTarget: null,
      }
    },
  }
})
vi.mock('@/components/device/ConnectMobileDialog', () => ({ default: () => null }))
vi.mock('@/pages/devices-kit/DevicesKitConnectPhone', () => ({
  DevicesKitConnectPhone: ({
    onConfigure,
    onSuccess,
  }: {
    onConfigure: () => void
    onSuccess: (result: { deviceId: string; label: string }) => void
  }) => (
    <div data-testid="mobile-flow">
      <button type="button" onClick={onConfigure}>
        Configure phone
      </button>
      <button
        type="button"
        onClick={() => onSuccess({ deviceId: 'phone-test', label: 'Test phone' })}
      >
        Register phone
      </button>
    </div>
  ),
}))
vi.mock('@/pages/devices-kit/DevicesKitPhoneComplete', () => ({
  DevicesKitPhoneComplete: ({ onDone }: { onDone: () => void }) => (
    <div data-testid="phone-complete">
      <button type="button" onClick={onDone}>
        Finish connection
      </button>
    </div>
  ),
}))
vi.mock('@/components/device/MobileSyncSettingsDialog', () => ({ default: () => null }))
vi.mock('@/pages/devices-kit/DevicesKitList', () => ({
  DevicesKitList: () => <div>Device list</div>,
}))
vi.mock('@/pages/devices-kit/DevicesKitDetail', () => ({ DevicesKitDetail: () => null }))
vi.mock('@/ui-kit', () => ({
  Sheet: ({ open, title, children, footer, done, onOpenChange }: SheetProps) =>
    open ? (
      <div role="dialog" aria-label={title}>
        {children}
        {footer}
        {done && (
          <button type="button" onClick={done.onClick}>
            {done.label}
          </button>
        )}
        <button type="button" onClick={() => onOpenChange(false)}>
          Dismiss sheet
        </button>
      </div>
    ) : null,
  MasterDetail: ({ list }: { list: ReactNode }) => list,
  TaskPanel: ({ title, children, back, actions }: TaskPanelProps) => (
    <section>
      <h2>{title}</h2>
      {children}
      {back && (
        <button type="button" disabled={back.disabled} onClick={back.onClick}>
          {back.label}
        </button>
      )}
      {actions}
    </section>
  ),
  SettingsGroup: ({ children }: { children: ReactNode }) => children,
  SettingsRow: ({ children }: { children: ReactNode }) => children,
  AlertDialog: () => null,
  Menu: ({ items, onAction }: MenuProps) => (
    <>
      <button type="button" onClick={() => onAction(items.find(item => item.id === 'join')!.id)}>
        Join space
      </button>
      <button type="button" onClick={() => onAction('phone')}>
        Connect phone
      </button>
    </>
  ),
  Button: ({ children, label, disabled, onClick, submitForm }: ButtonProps) => (
    <button
      type={submitForm ? 'submit' : 'button'}
      form={submitForm}
      disabled={disabled}
      onClick={onClick}
    >
      {children ?? label}
    </button>
  ),
  CodeField: ({ id, label, value, onValueChange, autoFocus }: CodeFieldProps) => (
    <input
      id={id}
      aria-label={label}
      value={value}
      onChange={event => onValueChange(event.target.value)}
      autoFocus={autoFocus}
    />
  ),
  FormField: ({ id, label, value, onValueChange, autoFocus, type }: FormFieldProps) => (
    <input
      id={id}
      aria-label={label}
      type={type}
      value={value}
      onChange={event => onValueChange(event.target.value)}
      autoFocus={autoFocus}
    />
  ),
  Text: ({ children }: { children: ReactNode }) => <span>{children}</span>,
  ProgressBar: ({ label }: { label: string }) => <div role="progressbar" aria-label={label} />,
  InlineNotice: ({ title, text }: InlineNoticeProps) => (
    <div>
      {title}
      {text}
    </div>
  ),
}))

function mount() {
  const devices = {
    open: true,
    selection: { kind: 'local' },
    invitation: { active: false },
    setOpen: vi.fn(),
    setSelection: vi.fn(),
    onClosed: vi.fn(),
  } as unknown as DevicesKitController
  render(
    <I18nextProvider i18n={i18n}>
      <DevicesKitSheet devices={devices} />
    </I18nextProvider>
  )
  fireEvent.click(screen.getByRole('button', { name: 'Join space' }))
  return devices
}

function fill() {
  fireEvent.change(screen.getByLabelText('Invitation code'), { target: { value: '012345' } })
  fireEvent.click(screen.getByRole('button', { name: 'Continue' }))
  fireEvent.change(screen.getByLabelText('New space passphrase'), {
    target: { value: 'test passphrase' },
  })
}

describe('device sheet switch-space navigation', () => {
  beforeEach(async () => {
    vi.clearAllMocks()
    await i18n.changeLanguage('en-US')
    api.switchSpace.mockResolvedValue({ status: 'pending', joinId: 'join-test' })
    api.cancelJoinSpace.mockResolvedValue({
      status: 'pending',
      joinId: 'join-test',
      cancelRequested: true,
    })
  })

  it('shows the phone flow inside the existing sheet and returns to devices', () => {
    mount()
    fireEvent.click(screen.getByRole('button', { name: 'Back to devices' }))
    const dialog = screen.getByRole('dialog')
    fireEvent.click(screen.getByRole('button', { name: 'Connect phone' }))
    expect(screen.getByTestId('mobile-flow')).toBeInTheDocument()
    expect(screen.getByRole('dialog')).toBe(dialog)
    expect(screen.queryByText('Device list')).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Dismiss sheet' }))
    expect(screen.getByText('Device list')).toBeInTheDocument()
  })

  it('replaces the device browser inside the same dialog and resets credentials on return', () => {
    const devices = mount()
    const dialog = screen.getByRole('dialog')
    expect(screen.queryByText('Device list')).not.toBeInTheDocument()
    expect(screen.getByLabelText('Invitation code')).toHaveFocus()
    fill()
    expect(screen.getByLabelText('New space passphrase')).toHaveFocus()
    expect(screen.getAllByRole('dialog')).toHaveLength(1)
    fireEvent.click(screen.getByRole('button', { name: 'Back' }))
    fireEvent.click(screen.getByRole('button', { name: 'Back to devices' }))
    expect(screen.getByRole('dialog')).toBe(dialog)
    expect(screen.getByText('Device list')).toBeInTheDocument()
    expect(devices.setOpen).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: 'Join space' }))
    expect(screen.getByLabelText('Invitation code')).toHaveValue('')
    fill()
    fireEvent.click(screen.getByRole('button', { name: 'Dismiss sheet' }))
    expect(screen.getByText('Device list')).toBeInTheDocument()
  })

  it('submits with Enter and cancels a pending admission without opening another dialog', async () => {
    mount()
    fill()
    fireEvent.submit(document.getElementById('switch-space-form')!)
    await screen.findByText('Waiting for confirmation')
    expect(api.switchSpace).toHaveBeenCalledWith({
      code: '012345',
      newPassphrase: 'test passphrase',
      preserveUnreadableHistory: false,
    })
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    await waitFor(() => expect(api.cancelJoinSpace).toHaveBeenCalledWith('join-test'))
    expect(screen.getAllByRole('dialog')).toHaveLength(1)
  })

  it('blocks dismissal while migration and final confirmation run', async () => {
    let resolve!: (value: unknown) => void
    api.switchSpace.mockReturnValue(
      new Promise(done => {
        resolve = done
      })
    )
    const devices = mount()
    fill()
    fireEvent.click(screen.getByRole('button', { name: 'Switch' }))
    fireEvent.click(screen.getByRole('button', { name: 'Dismiss sheet' }))
    expect(screen.getByText('Migrating clipboard history')).toBeInTheDocument()
    resolve({ status: 'processing', joinId: 'join-test' })
    await screen.findByText('Completing the switch')
    fireEvent.click(screen.getByRole('button', { name: 'Dismiss sheet' }))
    expect(screen.getByText('Completing the switch')).toBeInTheDocument()
    expect(devices.setOpen).not.toHaveBeenCalled()
  })

  it('keeps failure and retry in the same sheet', async () => {
    api.switchSpace.mockRejectedValue(new Error('test failure'))
    mount()
    fill()
    fireEvent.click(screen.getByRole('button', { name: 'Switch' }))
    await screen.findByText(/Switch space failed/)
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }))
    expect(screen.getByLabelText('Invitation code')).toHaveValue('012345')
    expect(screen.getAllByRole('dialog')).toHaveLength(1)
  })

  it('shows the switch result until Done returns to the device browser', async () => {
    api.switchSpace.mockResolvedValue({
      status: 'active',
      joinedSpace: { migratedRecords: 3, preservedUnreadableRecords: 0 },
    })
    mount()
    fill()
    fireEvent.click(screen.getByRole('button', { name: 'Switch' }))
    await screen.findByTestId('switch-space-success')
    expect(screen.queryByText('Device list')).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Done' }))
    expect(screen.getByText('Device list')).toBeInTheDocument()
  })

  it('keeps phone credentials in the task until completion and clears them on leaving', () => {
    mount()
    fireEvent.click(screen.getByRole('button', { name: 'Back to devices' }))
    fireEvent.click(screen.getByRole('button', { name: 'Connect phone' }))
    fireEvent.click(screen.getByRole('button', { name: 'Register phone' }))
    expect(screen.getByTestId('phone-complete')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Finish connection' }))
    expect(screen.getByText('Device list')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Connect phone' }))
    expect(screen.queryByTestId('phone-complete')).not.toBeInTheDocument()
  })
})
