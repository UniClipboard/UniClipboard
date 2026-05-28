import { RefreshCw } from 'lucide-react'
import React from 'react'
import { syncClipboardItems } from '@/api/clipboardItems'
import type { ClipboardStats } from '@/api/daemon/clipboard'
import { Button } from '@/components/ui/button'
import { createLogger } from '@/lib/logger'
import { useAppDispatch } from '@/store/hooks'
import { clearAllItems } from '@/store/slices/clipboardSlice'
import { formatFileSize } from '@/utils'

const log = createLogger('action-bar')

interface ActionBarProps {
  stats: ClipboardStats
  onSync?: () => void
}

const ActionBar: React.FC<ActionBarProps> = ({ stats, onSync }) => {
  const dispatch = useAppDispatch()

  // 处理清理所有剪贴板项
  const handleClearAll = async () => {
    if (window.confirm('确定要清理所有剪贴板项吗？')) {
      try {
        await dispatch(clearAllItems()).unwrap()
      } catch (err) {
        log.error({ err }, '清理剪贴板项失败')
      }
    }
  }

  // 处理立即同步
  const handleSync = async () => {
    try {
      log.info('开始同步剪贴板项')
      await syncClipboardItems()
      log.info('剪贴板项同步完成')

      // 调用父组件传递的同步成功回调
      if (onSync) {
        onSync()
      }
    } catch (err) {
      log.error({ err }, '同步剪贴板项失败')
      alert('同步失败，请稍后重试。')
    }
  }

  return (
    <footer className="absolute bottom-0 w-full glass-strong border-t border-border px-8 py-4 flex items-center justify-between z-10">
      <div className="text-sm text-muted-foreground flex items-center gap-2">
        <span className="font-medium text-foreground">已同步 {stats.totalItems} 项</span>
        <span>·</span>
        <span>已使用 {formatFileSize(stats.totalSize)}</span>
      </div>

      <div className="flex items-center gap-3">
        <Button variant="outline" size="sm" onClick={handleClearAll} className="rounded-lg">
          清理所有
        </Button>
        <Button
          size="sm"
          onClick={handleSync}
          className="rounded-lg bg-primary hover:bg-primary/90 shadow-lg shadow-primary/30"
        >
          <RefreshCw className="size-4 mr-2" />
          立即同步
        </Button>
      </div>
    </footer>
  )
}

export default ActionBar
