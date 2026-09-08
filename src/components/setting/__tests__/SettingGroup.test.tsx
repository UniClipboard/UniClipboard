import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { SettingGroup } from '@/components/setting/SettingGroup'

describe('SettingGroup', () => {
  it('names a native fieldset with its legend without adding a page heading', () => {
    render(
      <SettingGroup title="Startup">
        <input aria-label="Device name" />
      </SettingGroup>
    )
    const group = screen.getByRole('group', { name: 'Startup' })
    expect(group.tagName).toBe('FIELDSET')
    expect(group.firstElementChild?.tagName).toBe('LEGEND')
    expect(screen.queryByRole('heading')).toBeNull()
    expect(screen.getByRole('textbox')).toBeEnabled()
  })

  it('preserves untitled groups and supplied styles', () => {
    render(
      <SettingGroup className="mt-4">
        <button type="button">Save</button>
      </SettingGroup>
    )
    expect(screen.getByRole('group')).toHaveClass('min-w-0', 'mt-4')
    expect(document.querySelector('legend')).toBeNull()
    expect(screen.getByRole('button', { name: 'Save' })).toBeEnabled()
  })
})
