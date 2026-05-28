import { Copy, Download, FolderOpen, Loader2, RefreshCw, Trash2 } from 'lucide-react'
import React, { useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuShortcut,
  ContextMenuTrigger,
} from '@/components/ui/context-menu'
import { useEntryDelivery } from '@/hooks/useEntryDelivery'
import { useResendAction } from '@/hooks/useResendAction'
import { useAppSelector } from '@/store/hooks'
import {
  resolveEntryTransferStatus,
  selectEntryTransferStatus,
  selectTransferByEntryId,
} from '@/store/slices/fileTransferSlice'
import type { DisplayClipboardItem } from './ClipboardContent'

export interface FileContextMenuTransferStatus {
  isDownloaded: boolean
  isTransferring: boolean
  isStale?: boolean
  /**
   * 该 entry 是否含 partial(uniclip-missing://)文件。当 daemon 的
   * file_transfer projection 还没落到 status='cancelled'(NewContent 比
   * Cancelled event 早到的窗口),effectiveStatus 仍然是 undefined,
   * 此时单靠 effectiveStatus 判断会让 Copy 误开放——这条标志走
   * representation bytes 的 source of truth,作为兜底。
   */
  hasMissingFiles?: boolean
}

interface FileContextMenuProps {
  children: React.ReactNode
  itemId: string
  itemType: DisplayClipboardItem['type']
  transferStatus: FileContextMenuTransferStatus
  onCopy: (itemId: string) => void
  onDelete: (itemId: string) => void
  onSyncToClipboard: (itemId: string) => void
  onOpenFileLocation: (itemId: string) => void
}

const FileContextMenu: React.FC<FileContextMenuProps> = ({
  children,
  itemId,
  itemType,
  transferStatus,
  onCopy,
  onDelete,
  onSyncToClipboard,
  onOpenFileLocation,
}) => {
  const { isDownloaded, isTransferring, isStale, hasMissingFiles } = transferStatus
  const { t } = useTranslation()
  const entryStatus = useAppSelector(state => selectEntryTransferStatus(state, itemId))
  const transfer = useAppSelector(state => selectTransferByEntryId(state, itemId))
  // Resend 触发器 + sonner toast 副作用共用 hook。
  const resendAction = useResendAction()
  // Lazy gate:仅在用户实际打开右键菜单后才拉 delivery 视图,知道来源
  // 是 remote/historical 就把 Resend 菜单项隐藏掉,让 contextmenu 与
  // HoverCard popover 的 UX 一致(后者也只对 `source.tag === 'local'` 显示
  // resend)。延迟到 open 之后才查避免列表初始渲染 fan-out N 个 IPC,只
  // 对真正打开菜单的那一行付出代价。loading 或拉失败时退化到"按钮可
  // 见、信后端 typed error 兜底",仍能 toast 出 `notResendable.remoteOrigin`。
  const [menuOpen, setMenuOpen] = useState(false)
  const { delivery: menuDelivery } = useEntryDelivery(menuOpen ? itemId : null)
  const hideResend =
    menuDelivery?.source.tag === 'remote' || menuDelivery?.source.tag === 'historical'

  const isFile = itemType === 'file'
  const effectiveStatus = resolveEntryTransferStatus(entryStatus, transfer)

  // Copy 在以下场景全部 disable:
  // - 非 completed 的 file_transfer 状态(pending/transferring/failed/cancelled)
  // - hasMissingFiles=true 兜底(避免在 file_transfer projection 落库前的
  //   时间窗内把 uniclip-missing:// URI 写到系统剪贴板)
  const isCopyDisabledByTransfer =
    isFile &&
    ((effectiveStatus != null && effectiveStatus !== 'completed') || hasMissingFiles === true)
  const copyDisabledReason = isCopyDisabledByTransfer
    ? effectiveStatus === 'pending'
      ? t('clipboard.transfer.copyDisabled.pending')
      : effectiveStatus === 'transferring'
        ? t('clipboard.transfer.copyDisabled.transferring')
        : effectiveStatus === 'cancelled'
          ? t('clipboard.transfer.copyDisabled.cancelled')
          : t('clipboard.transfer.copyDisabled.failed')
    : null

  const showSyncAction = isFile && !isDownloaded && !isCopyDisabledByTransfer
  const showCopyAction = !isFile || isDownloaded || isCopyDisabledByTransfer

  return (
    <ContextMenu onOpenChange={setMenuOpen}>
      <ContextMenuTrigger asChild>{children}</ContextMenuTrigger>
      <ContextMenuContent className="w-52">
        {/* Sync to Clipboard (file not yet downloaded, no blocking transfer state) */}
        {showSyncAction && (
          <ContextMenuItem disabled={isTransferring} onClick={() => onSyncToClipboard(itemId)}>
            {isTransferring ? (
              <Loader2 className="mr-2 size-4 animate-spin" />
            ) : (
              <Download className="mr-2 size-4" />
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
            <Copy className="mr-2 size-4" />
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
          !hasMissingFiles &&
          effectiveStatus !== 'pending' &&
          effectiveStatus !== 'transferring' &&
          effectiveStatus !== 'failed' &&
          effectiveStatus !== 'cancelled' && (
            <>
              <ContextMenuItem onClick={() => onOpenFileLocation(itemId)}>
                <FolderOpen className="mr-2 size-4" />
                {t('clipboard.contextMenu.openFileLocation')}
              </ContextMenuItem>
              <ContextMenuSeparator />
            </>
          )}

        {/* Resend —— 用户主动重发到 pending/failed 的可信对端。已知
            remote/historical 来源时直接不渲染(与 HoverCard popover 的
            resendable 判断对齐);未知/loading 时退化到信后端 typed error
            兜底,本机无 payload / 已全 delivered 等都走 toast。 */}
        {!hideResend && (
          <>
            <ContextMenuItem
              disabled={resendAction.isEntryInFlight(itemId)}
              onClick={() => void resendAction.resendAll(itemId)}
            >
              {resendAction.isEntryInFlight(itemId) ? (
                <Loader2 className="mr-2 size-4 animate-spin" />
              ) : (
                <RefreshCw className="mr-2 size-4" />
              )}
              {resendAction.isEntryInFlight(itemId)
                ? t('clipboard.contextMenu.resending')
                : t('clipboard.contextMenu.resend')}
            </ContextMenuItem>

            <ContextMenuSeparator />
          </>
        )}

        {/* Delete - always available for every transfer state */}
        <ContextMenuItem
          className="text-destructive focus:text-destructive"
          onClick={() => onDelete(itemId)}
        >
          <Trash2 className="mr-2 size-4" />
          {t('clipboard.contextMenu.delete')}
          <ContextMenuShortcut>D</ContextMenuShortcut>
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  )
}

export default FileContextMenu
