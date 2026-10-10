import { render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { getDeviceMeta } from '@/api/runtime'
import DevProfileIndicator from '@/components/DevProfileIndicator'

vi.mock('@/api/runtime', () => ({
  getDeviceMeta: vi.fn(),
}))

const mockGetDeviceMeta = vi.mocked(getDeviceMeta)

describe('DevProfileIndicator', () => {
  beforeEach(() => {
    mockGetDeviceMeta.mockReset()
  })

  it('shows the current profile in development mode', async () => {
    mockGetDeviceMeta.mockResolvedValue({
      deviceId: 'device-a',
      deviceRole: 'gui-host',
      platform: 'macos',
      appVersion: '1.2.3',
      appChannel: 'dev',
      runtimeProfile: 'alpha',
      developmentMode: true,
    })

    render(<DevProfileIndicator />)

    expect(await screen.findByLabelText('Development profile: alpha')).toBeVisible()
    expect(screen.getByText('alpha')).toBeVisible()
  })

  it('stays hidden outside development mode', async () => {
    mockGetDeviceMeta.mockResolvedValue({
      deviceId: 'device-a',
      deviceRole: 'gui-host',
      platform: 'macos',
      appVersion: '1.2.3',
      appChannel: 'stable',
      runtimeProfile: 'default',
      developmentMode: false,
    })

    const { container } = render(<DevProfileIndicator />)

    await waitFor(() => expect(mockGetDeviceMeta).toHaveBeenCalledOnce())
    expect(container).toBeEmptyDOMElement()
  })
})
