import { render, screen } from '@testing-library/react'
import { describe, it, expect, vi } from 'vitest'
import DeviceSettingsPanel from '../DeviceSettingsPanel'
import i18n from '@/i18n'

vi.mock('framer-motion', () => ({
  motion: {
    div: ({ children, className, ...props }: any) => (
      <div className={className} {...props}>
        {children}
      </div>
    ),
  },
  AnimatePresence: ({ children }: any) => <>{children}</>,
}))

describe('DeviceSettingsPanel', () => {
  const defaultProps = {
    deviceId: 'test-device-id',
    deviceName: 'Test Device',
  }

  it('renders sync rules section', () => {
    render(<DeviceSettingsPanel {...defaultProps} />)
    expect(screen.getByText(i18n.t('devices.settings.sync.title'))).toBeInTheDocument()
    expect(
      screen.getByText(i18n.t('devices.settings.sync.rules.autoSync.title'))
    ).toBeInTheDocument()
    expect(
      screen.getByText(i18n.t('devices.settings.sync.rules.syncText.title'))
    ).toBeInTheDocument()
  })

  it('renders permissions section', () => {
    render(<DeviceSettingsPanel {...defaultProps} />)
    expect(screen.getByText(i18n.t('devices.settings.permissions.title'))).toBeInTheDocument()
    expect(
      screen.getByText(i18n.t('devices.settings.permissions.items.readClipboard'))
    ).toBeInTheDocument()
    expect(
      screen.getByText(i18n.t('devices.settings.permissions.items.writeClipboard'))
    ).toBeInTheDocument()
  })
})
