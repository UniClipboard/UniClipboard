import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { useRef, useState } from 'react'
import { describe, expect, it, vi } from 'vitest'
import HistoryTagCustomColor from '../HistoryTagCustomColor'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}))

function Harness({ onSave }: { onSave: (color: string) => void }) {
  const anchor = useRef<HTMLButtonElement>(null)
  const [open, setOpen] = useState(true)
  return (
    <>
      <button ref={anchor} type="button">
        anchor
      </button>
      <HistoryTagCustomColor
        open={open}
        onOpenChange={setOpen}
        anchor={anchor}
        color="orange"
        onSave={onSave}
      />
    </>
  )
}

describe('HistoryTagCustomColor', () => {
  it('saves the picked color once, when the picker closes', async () => {
    const user = userEvent.setup()
    const onSave = vi.fn()
    render(<Harness onSave={onSave} />)

    await user.click(await screen.findByRole('radio', { name: 'history.tags.colors.green' }))
    await user.type(
      screen.getByRole('textbox', { name: 'history.tags.hexColor' }),
      '{Control>}a{/Control}#c0467a'
    )
    expect(onSave).not.toHaveBeenCalled()

    await user.keyboard('{Escape}')
    expect(onSave).toHaveBeenCalledTimes(1)
    expect(onSave).toHaveBeenCalledWith('#c0467a')
  })

  it('saves nothing when the color did not change', async () => {
    const user = userEvent.setup()
    const onSave = vi.fn()
    render(<Harness onSave={onSave} />)

    await screen.findByRole('radio', { name: 'history.tags.colors.green' })
    await user.keyboard('{Escape}')
    expect(onSave).not.toHaveBeenCalled()
  })
})
