import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import PanelItem from '@/quick-panel/components/PanelItem'

const text = '\r\n  \n---\r\n \t\r\ntitle: "Scroll Animation"\n正文\n'
const expected = '--- ↵ title: "Scroll Animation" ↵ 正文'

describe('quick panel text summaries', () => {
  it('quick panel shows the same summary and still selects the original entry', () => {
    const onSelect = vi.fn()
    const { container } = render(
      <PanelItem
        item={{
          id: 'summary',
          type: 'text',
          preview: text,
          activeTime: Date.now(),
          isUnavailable: false,
        }}
        itemRefs={new Map()}
        index={2}
        isSelected={true}
        hoverDisabled={false}
        onSelect={onSelect}
        onHover={vi.fn()}
        onContextMenu={vi.fn()}
        isFavorited={false}
      />
    )
    expect(container.textContent).toContain(expected)
    fireEvent.click(screen.getByRole('option'))
    expect(onSelect).toHaveBeenCalledWith(2, false)
  })
})
