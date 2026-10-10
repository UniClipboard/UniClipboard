import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { Filter } from '@/api/clipboardItems'
import { buildChips } from '../composite-search-model'
import SearchFacetRow from '../SearchFacetRow'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}))

describe('SearchFacetRow', () => {
  it('badges the tag facet with the number of selected tags', () => {
    const chips = buildChips({
      t: key => key,
      sourceOptions: [],
      tagOptions: [],
      current: {
        type: Filter.Image,
        tag: 'link,code,image',
        source: null,
        time: 'all_time',
        extension: null,
      },
    })
    render(<SearchFacetRow chips={chips} onSeedDimension={vi.fn()} onClearAll={vi.fn()} />)

    expect(
      screen.getByRole('button', { name: /history\.composite\.dimension\.tag/ })
    ).toHaveTextContent('3')
    expect(
      screen.getByRole('button', { name: /history\.composite\.dimension\.type/ })
    ).toHaveTextContent('1')
  })
})
