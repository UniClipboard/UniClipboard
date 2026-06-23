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

describe('ClipboardPreviewInfo', () => {
  it('renders file count and combined size for file entries', () => {
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
})
