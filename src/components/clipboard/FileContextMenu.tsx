import { Copy, Download, FolderOpen, Loader2, RefreshCw, Trash2 } from 'lucide-react'
import React from 'react'
import { useTranslation } from 'react-i18next'
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuShortcut,
  ContextMenuTrigger,
} from '@/components/ui/context-menu'
import { useResendAction } from '@/hooks/useResendAction'
import { useAppSelector } from '@/store/hooks'
import {
  resolveEntryTransferStatus,
  selectEntryTransferStatus,
  selectTransferByEntryId,
} from '@/store/slices/fileTransferSlice'
import type { DisplayClipboardItem } from './ClipboardContent'

interface FileContextMenuProps {
  children: React.ReactNode
  itemId: string
  itemType: DisplayClipboardItem['type']
  isDownloaded: boolean
  isTransferring: boolean
  isStale?: boolean
  onCopy: (itemId: string) => void
  onDelete: (itemId: string) => void
  onSyncToClipboard: (itemId: string) => void
  onOpenFileLocation: (itemId: string) => void
}

const FileContextMenu: React.FC<FileContextMenuProps> = ({
  children,
  itemId,
  itemType,
  isDownloaded,
  isTransferring,
  isStale,
  onCopy,
  onDelete,
  onSyncToClipboard,
  onOpenFileLocation,
}) => {
  const { t } = useTranslation()
  const entryStatus = useAppSelector(state => selectEntryTransferStatus(state, itemId))
  const transfer = useAppSelector(state => selectTransferByEntryId(state, itemId))
  // Resend 走 in-process facade (`AppFacade::resend_entry`),按钮始终显示;
  // 远端来源 / 已全 delivered / 本机无 payload 等情况由后端 typed error 兜底,
  // hook 内 toast 翻译给用户。这是有意"信后端做 gate"的取舍 —— 列表层若
  // 自己拉 delivery 做 enable check 会让初始渲染 fan-out N 个 IPC 请求。
  const resendAction = useResendAction()

  const isFile = itemType === 'file'
  const effectiveStatus = resolveEntryTransferStatus(entryStatus, transfer)

  // Copy is disabled for non-completed file entries (pending, transferring, failed)
  const isCopyDisabledByTransfer =
    isFile && effectiveStatus != null && effectiveStatus !== 'completed'
  const copyDisabledReason = isCopyDisabledByTransfer
    ? effectiveStatus === 'pending'
      ? t('clipboard.transfer.copyDisabled.pending')
      : effectiveStatus === 'transferring'
        ? t('clipboard.transfer.copyDisabled.transferring')
        : t('clipboard.transfer.copyDisabled.failed')
    : null

  const showSyncAction = isFile && !isDownloaded && !isCopyDisabledByTransfer
  const showCopyAction = !isFile || isDownloaded || isCopyDisabledByTransfer

  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>{children}</ContextMenuTrigger>
      <ContextMenuContent className="w-52">
        {/* Sync to Clipboard (file not yet downloaded, no blocking transfer state) */}
        {showSyncAction && (
          <ContextMenuItem disabled={isTransferring} onClick={() => onSyncToClipboard(itemId)}>
            {isTransferring ? (
              <Loader2 className="mr-2 h-4 w-4 animate-spin" />
            ) : (
              <Download className="mr-2 h-4 w-4" />
            )}
            {isTransferring
              ? t('clipboard.contextMenu.syncing')
              : t('clipboard.contextMenu.syncToClipboard')}
          </ContextMenuItem>
        )}

        {/* Copy (disabled for non-completed file transfers) */}
        {showCopyAction && (
          <ContextMenuItem
            disabled={isCopyDisabledByTransfer || (isFile && isStale)}
            aria-disabled={isCopyDisabledByTransfer || (isFile && isStale)}
            onClick={() => !isCopyDisabledByTransfer && !isStale && onCopy(itemId)}
          >
            <Copy className="mr-2 h-4 w-4" />
            {copyDisabledReason
              ? copyDisabledReason
              : isFile && isStale
                ? t('clipboard.contextMenu.fileDeleted', 'File deleted')
                : t('clipboard.contextMenu.copy')}
            {!isCopyDisabledByTransfer && !isStale && <ContextMenuShortcut>C</ContextMenuShortcut>}
          </ContextMenuItem>
        )}

        <ContextMenuSeparator />

        {/* Open File Location (file type, downloaded, completed transfer) */}
        {isFile &&
          isDownloaded &&
          effectiveStatus !== 'pending' &&
          effectiveStatus !== 'transferring' &&
          effectiveStatus !== 'failed' && (
            <>
              <ContextMenuItem onClick={() => onOpenFileLocation(itemId)}>
                <FolderOpen className="mr-2 h-4 w-4" />
                {t('clipboard.contextMenu.openFileLocation')}
              </ContextMenuItem>
              <ContextMenuSeparator />
            </>
          )}

        {/* Resend —— 用户主动重发到 pending/failed 的可信对端;远端来源 /
            本机无 payload 等情况后端会返回 typed error 走 toast 兜底。 */}
        <ContextMenuItem
          disabled={resendAction.entryInFlight}
          onClick={() => void resendAction.resendAll(itemId)}
        >
          {resendAction.entryInFlight ? (
            <Loader2 className="mr-2 h-4 w-4 animate-spin" />
          ) : (
            <RefreshCw className="mr-2 h-4 w-4" />
          )}
          {resendAction.entryInFlight
            ? t('clipboard.contextMenu.resending')
            : t('clipboard.contextMenu.resend')}
        </ContextMenuItem>

        <ContextMenuSeparator />

        {/* Delete - always available for every transfer state */}
        <ContextMenuItem
          className="text-destructive focus:text-destructive"
          onClick={() => onDelete(itemId)}
        >
          <Trash2 className="mr-2 h-4 w-4" />
          {t('clipboard.contextMenu.delete')}
          <ContextMenuShortcut>D</ContextMenuShortcut>
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  )
}

export default FileContextMenu
