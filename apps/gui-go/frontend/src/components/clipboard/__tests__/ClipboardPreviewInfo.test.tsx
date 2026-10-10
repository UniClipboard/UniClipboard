import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import i18n from '@/i18n'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'
import ClipboardPreviewInfo from '../ClipboardPreviewInfo'

function createFileItem(): DisplayClipboardItem {
  return {
    id: 'entry-files',
    type: 'file',
    activeTime: Date.now(),
    content: {
      file_names: ['first.zip', 'second.zip'],
      file_sizes: [1024, 2048],
    },
  }
}

function createCodeItem(text = 'const answer = 42'): DisplayClipboardItem {
  return {
    id: 'entry-code',
    type: 'text',
    activeTime: Date.now(),
    contentTags: ['code'],
    content: {
      display_text: text,
      has_detail: false,
      size: text.length,
      char_count: text.length,
    },
  }
}

describe('ClipboardPreviewInfo', () => {
  it('renders file count and combined size for file entries', async () => {
    render(
      <ClipboardPreviewInfo
        item={createFileItem()}
        preview={null}
        imageDimensions={null}
        delivery={null}
      />
    )

    expect(screen.getByText(i18n.t('header.filters.file'))).toBeInTheDocument()
    expect(
      screen.getByText(i18n.t('clipboard.preview.filesCount', { count: 2 }))
    ).toBeInTheDocument()
    expect(screen.getByText('3.00 KB')).toBeInTheDocument()
  })

  it('renders nothing when no item is selected', () => {
    const { container } = render(
      <ClipboardPreviewInfo item={null} preview={null} imageDimensions={null} delivery={null} />
    )

    expect(container).toBeEmptyDOMElement()
  })

  it('keeps text as the type and displays code once as a tag', () => {
    render(
      <ClipboardPreviewInfo
        item={createCodeItem()}
        preview={null}
        imageDimensions={null}
        delivery={null}
      />
    )

    expect(screen.queryByRole('button')).not.toBeInTheDocument()
    expect(screen.getAllByText(i18n.t('history.type.code'))).toHaveLength(1)
    expect(screen.getByText(i18n.t('header.filters.text'))).toBeInTheDocument()
  })

  it('shows the full code preview line count without a trailing phantom line', async () => {
    const text = 'const first = 1\nconst second = 2\n'
    render(
      <ClipboardPreviewInfo
        item={createCodeItem('const first = 1')}
        preview={{
          entryId: 'entry-code',
          contentType: 'text',
          sizeBytes: text.length,
          textContent: text,
        }}
        imageDimensions={null}
        delivery={null}
      />
    )

    expect(
      screen.getByText(i18n.t('clipboard.preview.linesCount', { count: 2 }))
    ).toBeInTheDocument()
  })
})
