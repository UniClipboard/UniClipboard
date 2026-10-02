import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { LazyMotion, MotionConfig, domMax } from 'framer-motion'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { DeviceNameSettings } from '@/components/setting/general/DeviceNameSettings'
import { StartupSettings } from '@/components/setting/general/StartupSettings'
import { useSetting } from '@/hooks/useSetting'
import { makeBaseSettings } from '@/test/fixtures/settings'
import type { SettingContextType } from '@/types/setting'

vi.mock('@/hooks/useSetting', () => ({ useSetting: vi.fn() }))
vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }) }))
const save = vi.fn()
beforeEach(() => {
  save.mockReset()
  const context = {
    setting: makeBaseSettings({ general: { deviceName: 'Computer', startupMode: 'normal' } }),
    updateGeneralSetting: save,
    updateAutostart: vi.fn().mockResolvedValue(undefined),
  } as unknown as SettingContextType
  save.mockImplementation(async patch => {
    if (context.setting)
      context.setting = { ...context.setting, general: { ...context.setting.general, ...patch } }
  })
  vi.mocked(useSetting).mockImplementation(() => context)
})

describe('general settings clarity', () => {
  it('shows the current startup summary and explains alternatives in the menu', async () => {
    const user = userEvent.setup()
    render(
      <LazyMotion features={domMax}>
        <MotionConfig skipAnimations>
          <StartupSettings />
        </MotionConfig>
      </LazyMotion>
    )
    expect(screen.getByText('settings.sections.general.startupMode.summaries.normal')).toBeVisible()
    expect(screen.queryByText('settings.sections.general.startupMode.description')).toBeNull()
    expect(screen.queryByRole('textbox')).toBeNull()
    await user.click(
      screen.getByRole('combobox', { name: 'settings.sections.general.startupMode.label' })
    )
    const option = await screen.findByRole('option', {
      name: 'settings.sections.general.startupMode.options.silent',
    })
    await waitFor(() =>
      expect(
        within(option).getByText('settings.sections.general.startupMode.summaries.silent')
      ).toBeVisible()
    )
    await user.click(option)
    await waitFor(() => expect(save).toHaveBeenCalledWith({ startupMode: 'silent' }))
    expect(
      within(
        screen.getByRole('group', { name: 'settings.sections.general.startupTitle' })
      ).getByText('settings.sections.general.startupMode.summaries.silent')
    ).toBeVisible()
  })

  it('preserves blur-to-save behavior for the separate device name row', async () => {
    const user = userEvent.setup()
    render(<DeviceNameSettings />)
    const input = screen.getByRole('textbox', {
      name: 'settings.sections.general.deviceName.label',
    })
    await user.clear(input)
    await user.type(input, 'Work computer')
    expect(save).not.toHaveBeenCalled()
    await user.tab()
    await waitFor(() => expect(save).toHaveBeenCalledWith({ deviceName: 'Work computer' }))
  })
})
