import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import * as storageApi from '@/api/storage'
import { UpgradeBackupGroup } from '@/components/setting/UpgradeBackupGroup'
import i18n from '@/i18n'

vi.mock('@/api/storage', () => ({
  listUpgradeBackups: vi.fn(),
  deleteUpgradeBackup: vi.fn(),
}))

vi.mock('@/components/ui/toast', () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}))

const listUpgradeBackups = vi.mocked(storageApi.listUpgradeBackups)
const deleteUpgradeBackup = vi.mocked(storageApi.deleteUpgradeBackup)

const backup = {
  id: 'f2aa5c89-471d-4c87-9d21-6808b57075ba',
  createdAtMs: Date.UTC(2026, 8, 13, 4, 30),
  sourceProduct: '0.19.3',
  sourceEngine: '1.0.0',
  targetProduct: '1.0.0',
  targetEngine: '1.1.0',
  sizeBytes: 2 * 1024 * 1024,
}

beforeAll(async () => {
  await i18n.changeLanguage('zh-CN')
})

beforeEach(() => {
  vi.clearAllMocks()
  listUpgradeBackups.mockResolvedValue([backup])
  deleteUpgradeBackup.mockResolvedValue(undefined)
})

afterEach(cleanup)

describe('UpgradeBackupGroup', () => {
  it('shows completed backups with version and size', async () => {
    render(<UpgradeBackupGroup />)

    expect(await screen.findByText('版本 0.19.3 → 1.0.0 · 2.00 MB')).toBeInTheDocument()
    expect(screen.getByText('已保留 1 / 5 份')).toBeInTheDocument()
  })

  it('requires confirmation, deletes the selected backup, and updates the list', async () => {
    const user = userEvent.setup()
    render(<UpgradeBackupGroup />)

    await user.click(await screen.findByRole('button', { name: /删除.+的备份/ }))
    expect(await screen.findByRole('alertdialog')).toHaveTextContent('删除这份升级备份？')

    await user.click(screen.getByRole('button', { name: '删除备份' }))

    await waitFor(() => {
      expect(deleteUpgradeBackup).toHaveBeenCalledWith(backup.id, true)
    })
    expect(await screen.findByText('还没有升级备份。')).toBeInTheDocument()
  })

  it('offers a retry after a loading failure', async () => {
    listUpgradeBackups.mockRejectedValueOnce(new Error('offline')).mockResolvedValueOnce([])
    const user = userEvent.setup()
    render(<UpgradeBackupGroup />)

    await user.click(await screen.findByRole('button', { name: '重试' }))

    expect(await screen.findByText('还没有升级备份。')).toBeInTheDocument()
    expect(listUpgradeBackups).toHaveBeenCalledTimes(2)
  })
})
