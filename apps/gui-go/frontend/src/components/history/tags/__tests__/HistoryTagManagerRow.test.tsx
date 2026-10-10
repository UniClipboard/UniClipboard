import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import type { LibraryTag } from '../history-tag-library'
import HistoryTagManagerRow from '../HistoryTagManagerRow'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}))

function renderRow(tag: LibraryTag, inSidebar = false) {
  const props = {
    onSetInSidebar: vi.fn(),
    onSetColor: vi.fn(),
    onToggleChecked: vi.fn(),
    onRename: vi.fn(),
    onMerge: vi.fn(),
    onDelete: vi.fn(),
    onShowItems: vi.fn(),
  }
  render(
    <HistoryTagManagerRow
      tag={tag}
      similar={null}
      mergeTargets={[]}
      inSidebar={inSidebar}
      checked={false}
      anyChecked={false}
      {...props}
    />
  )
  return props
}

const link: LibraryTag = {
  tagId: 'link',
  name: 'Link',
  entryCount: 3,
  createdAtMs: 0,
  builtin: true,
}
const deploy: LibraryTag = {
  tagId: 't-deploy',
  name: 'deploy',
  entryCount: 14,
  createdAtMs: 0,
  builtin: false,
}

describe('HistoryTagManagerRow', () => {
  it('toggles whether the sidebar shows the tag', async () => {
    const user = userEvent.setup()
    const props = renderRow(deploy, true)

    await user.click(screen.getByRole('switch', { name: 'history.tags.showInSidebar' }))
    expect(props.onSetInSidebar).toHaveBeenCalledWith('t-deploy', false)
  })

  it('offers a builtin tag no rename, merge, delete or check box', async () => {
    const user = userEvent.setup()
    renderRow(link)

    expect(screen.queryByRole('checkbox')).toBeNull()
    await user.click(screen.getByRole('button', { name: 'history.tags.actionsFor' }))
    expect(await screen.findByText('history.tags.color')).toBeInTheDocument()
    expect(screen.getByText('history.tags.showItems')).toBeInTheDocument()
    expect(screen.getByText('history.tags.builtinHint')).toBeInTheDocument()
    for (const action of ['history.tags.rename', 'history.tags.mergeInto', 'history.tags.delete']) {
      expect(screen.queryByText(action)).toBeNull()
    }
  })

  it('offers a local tag every action', async () => {
    const user = userEvent.setup()
    renderRow(deploy)

    expect(screen.getByRole('checkbox')).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: 'history.tags.actionsFor' }))
    for (const action of ['history.tags.rename', 'history.tags.color', 'history.tags.delete']) {
      expect(await screen.findByText(action)).toBeInTheDocument()
    }
  })
})
