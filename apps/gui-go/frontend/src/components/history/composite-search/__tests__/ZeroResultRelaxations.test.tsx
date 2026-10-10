import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { Clock, Image as ImageIcon } from 'lucide-react'
import { describe, expect, it, vi } from 'vitest'
import ZeroResultRelaxations from '../ZeroResultRelaxations'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      opts ? `${key}:${JSON.stringify(opts)}` : key,
  }),
}))

describe('ZeroResultRelaxations', () => {
  it('removes the chosen filter and disables ones that would still find nothing', async () => {
    const user = userEvent.setup()
    const onRemove = vi.fn()
    render(
      <ZeroResultRelaxations
        relaxations={[
          {
            chip: { dimension: 'type', label: 'Image', icon: ImageIcon, valueCount: 1 },
            count: 12,
          },
          { chip: { dimension: 'time', label: 'Today', icon: Clock, valueCount: 1 }, count: 0 },
        ]}
        onRemove={onRemove}
      />
    )

    const image = screen.getByRole('button', { name: /"filter":"Image"/ })
    const today = screen.getByRole('button', { name: /"filter":"Today"/ })
    expect(image).toHaveTextContent('"count":12')
    expect(today).toBeDisabled()

    await user.click(image)
    expect(onRemove).toHaveBeenCalledWith('type')
  })
})
