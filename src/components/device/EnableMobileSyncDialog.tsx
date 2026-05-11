/**
 * # 为什么需要这个组件
 *
 * 简化首次添加移动设备的流程。在 phase 6 之前,用户点 `+ Add` 必须先打开
 * Configure 抽屉 → 开 enabled 开关 → 开 lan listener 开关 → 弹安全告警 →
 * 看 restart banner → 点重启 → 重启完成后再回来点 Add → 终于填 label 拿到
 * 凭据 —— 四个步骤、两次弹窗、一次重启,极其反直觉。
 *
 * 现在(phase 6)Panel 把 `+ Add` 改成"首次配置入口":未配置时点击就弹本
 * 对话框, 一次确认 → 后台原子开两个开关 + 安全告警合并到本 dialog →
 * 立刻进入填 label 表单。零进程重启, 零页面跳转。
 *
 * # 对外能力
 *
 * 受控对话框组件。`open` 控制可见性;确认 button 调
 * `updateMobileSyncSettings({ enabled: true, lanListenEnabled: true })`,
 * 成功后 `onSuccess()` 回调让父组件接管(典型:立刻打开
 * AddMobileSyncDeviceDialog)。失败时 toast.error。
 *
 * # 内部实现要点
 *
 * - port / bindIp 走 daemon 默认值(42720 / 0.0.0.0),用户后续可在
 *   Configure 抽屉里调整。这里不暴露这两个字段是有意 —— 首次添加场景
 *   下决策最少化, 默认值能让 90% 的用户跑通
 * - 提交 in-flight 期间禁用所有按钮 + onOpenChange 拒绝关闭, 防止双击或
 *   误关导致 settings 落盘但 dialog 状态错乱
 */

import { Loader2 } from 'lucide-react'
import React, { useCallback, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  isMobileSyncError,
  updateMobileSyncSettings,
  type MobileSyncError,
} from '@/api/tauri-command/mobile_sync'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { toast } from '@/components/ui/toast'
import { createLogger } from '@/lib/logger'

const log = createLogger('enable-mobile-sync-dialog')

/** 默认 LAN port,与 daemon 装配期一次性读 settings 时的兜底完全一致。 */
const DEFAULT_LAN_PORT = 42720

interface Props {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** 成功开启后回调。父组件典型做法:立刻打开 AddMobileSyncDeviceDialog。 */
  onSuccess: () => void
}

const EnableMobileSyncDialog: React.FC<Props> = ({ open, onOpenChange, onSuccess }) => {
  const { t } = useTranslation()
  const [submitting, setSubmitting] = useState(false)

  const handleConfirm = useCallback(async () => {
    setSubmitting(true)
    try {
      const result = await updateMobileSyncSettings({
        enabled: true,
        lanListenEnabled: true,
      })
      // 写盘成功不代表 listener 起来了 —— daemon 端 lifecycle adapter 可能
      // 报 bind 失败(端口占用 / 权限 / IP 不可用),那种情况下 onSuccess 会
      // 把用户带进 AddMobileSyncDeviceDialog → 填完 label → 拿凭据 →
      // iPhone 连不上,体验更差。在这里硬阻断,toast 告诉用户原因,让他们
      // 去 Configure 抽屉里换端口。
      if (result.lanListenerBindError) {
        log.warn(
          { reason: result.lanListenerBindError },
          'settings saved but LAN listener bind failed; abort onSuccess'
        )
        toast.error(
          t('devices.mobileSync.feedback.applyFailed', {
            message: result.lanListenerBindError,
          })
        )
        return
      }
      onSuccess()
      onOpenChange(false)
    } catch (err) {
      log.error({ err }, 'failed to enable mobile sync')
      toast.error(translateApplyError(t, err))
    } finally {
      setSubmitting(false)
    }
  }, [onOpenChange, onSuccess, t])

  return (
    <AlertDialog
      open={open}
      onOpenChange={next => {
        if (!submitting) onOpenChange(next)
      }}
    >
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{t('devices.mobileSync.enableConfirm.title')}</AlertDialogTitle>
          <AlertDialogDescription>
            {t('devices.mobileSync.enableConfirm.body', { port: DEFAULT_LAN_PORT })}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel disabled={submitting}>
            {t('devices.mobileSync.enableConfirm.cancel')}
          </AlertDialogCancel>
          <AlertDialogAction
            onClick={e => {
              e.preventDefault()
              void handleConfirm()
            }}
            disabled={submitting}
          >
            {submitting && <Loader2 className="h-4 w-4 animate-spin" />}
            {submitting
              ? t('devices.mobileSync.enableConfirm.enabling')
              : t('devices.mobileSync.enableConfirm.confirm')}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}

function translateApplyError(t: ReturnType<typeof useTranslation>['t'], err: unknown): string {
  if (isMobileSyncError(err)) {
    const e = err as MobileSyncError
    const message = (e as { message?: string }).message ?? e.code
    return t('devices.mobileSync.feedback.applyFailed', { message })
  }
  const message = err instanceof Error ? err.message : String(err)
  return t('devices.mobileSync.feedback.applyFailed', { message })
}

export default EnableMobileSyncDialog
