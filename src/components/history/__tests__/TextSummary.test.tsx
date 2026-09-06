import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import HistoryCardContent from '@/components/history/history-card/HistoryCardContent'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'
import PanelItem from '@/quick-panel/components/PanelItem'

const text = '\r\n  \n---\r\n \t\r\ntitle: "Scroll Animation"\n正文\n'
const expected = '--- ↵ title: "Scroll Animation" ↵ 正文'

describe('list text summaries', () => {
  it.each(['text', 'richtext', 'code', 'unloaded', 'unloaded-code'])(
    '%s joins lines and skips blank lines',
    kind => {
      const unloaded = kind.startsWith('unloaded')
      const item = {
        id: 'summary',
        type: kind === 'richtext' ? 'richtext' : 'text',
        content: unloaded ? null : { display_text: text, char_count: text.length },
        textPreview: text,
        contentTags: kind.includes('code') ? ['code'] : [],
        activeTime: 1,
      } as DisplayClipboardItem
      const { container } = render(<HistoryCardContent item={item} />)
      expect(container.textContent).toBe(expected)
      expect(item.textPreview).toBe(text)
    }
  )

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
