import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import { SETTINGS_CATEGORIES } from '@/components/setting/settings-config'
import SettingsPageHeader from '@/components/setting/SettingsPageHeader'
import en from '@/i18n/locales/en-US.json'
import zh from '@/i18n/locales/zh-CN.json'
import SettingContentLayout from '@/layouts/SettingContentLayout'

vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }) }))

describe('shared settings page header', () => {
  it.each(SETTINGS_CATEGORIES.map(category => category.id))(
    'provides a title and description for %s',
    category => {
      const header = <SettingsPageHeader category={category} />
      render(
        <SettingContentLayout header={header}>
          <div>Existing settings</div>
        </SettingContentLayout>
      )
      expect(screen.getAllByRole('heading', { level: 1 })).toHaveLength(1)
      expect(screen.getByRole('heading', { name: `settings.categories.${category}` })).toBeVisible()
      expect(screen.getByText(`settings.pageDescriptions.${category}`)).toBeVisible()
      for (const locale of [zh, en]) {
        expect(
          locale.settings.pageDescriptions[category as keyof typeof zh.settings.pageDescriptions]
        ).toBeTruthy()
      }
    }
  )

  it('updates the header without resetting existing controls', async () => {
    const header = <SettingsPageHeader category="general" />
    const nextHeader = <SettingsPageHeader category="network" />
    const { rerender } = render(
      <SettingContentLayout header={header}>
        <input aria-label="Existing setting" defaultValue="" />
      </SettingContentLayout>
    )
    await userEvent.setup().type(screen.getByRole('textbox'), 'keep this value')
    rerender(
      <SettingContentLayout header={nextHeader}>
        <input aria-label="Existing setting" defaultValue="" />
      </SettingContentLayout>
    )
    expect(screen.getByRole('heading', { name: 'settings.categories.network' })).toBeVisible()
    expect(screen.getByRole('textbox')).toHaveValue('keep this value')
    expect(screen.getAllByTestId('settings-page-header')).toHaveLength(1)
  })
})
