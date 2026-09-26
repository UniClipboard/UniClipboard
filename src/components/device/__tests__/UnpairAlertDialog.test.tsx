import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import UnpairAlertDialog from '@/components/device/UnpairAlertDialog'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, options?: { deviceName?: string }) => options?.deviceName ?? key,
  }),
}))

describe('UnpairAlertDialog', () => {
  it('waits for the owner to close after the async removal result', () => {
    const onConfirm = vi.fn()
    const onOpenChange = vi.fn()
    render(
      <UnpairAlertDialog
        open
        onOpenChange={onOpenChange}
        deviceName="Office PC"
        busy={false}
        onConfirm={onConfirm}
      />
    )

    fireEvent.click(screen.getByRole('button', { name: 'devices.list.actions.unpair' }))

    expect(onConfirm).toHaveBeenCalledOnce()
    expect(onOpenChange).not.toHaveBeenCalled()
    expect(screen.getByRole('alertdialog')).toBeVisible()
  })
})
