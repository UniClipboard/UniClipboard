/**
 * MobileSyncBaseUrlChip —— mobile sync 流程里复用的"服务地址 chip"。
 *
 * 单 IP 退化为只读 chip + 复制按钮, 多 IP 走 dropdown + 复制按钮。
 * 既被注册成功后的 `MobileSyncCredentialModal` 用, 也被设备卡片的
 * `MobileSyncDeviceDialog` 用 —— 两边共享同一控件以保持视觉与行为一致。
 *
 * 注意: 切换 host 不写回 daemon settings 也不重启 listener — daemon 永远
 * bind `0.0.0.0:<lan_port>`, 这里的 host 只影响"对外展示 / 写入 QR / 复制"。
 */

import { Check, Copy } from 'lucide-react'
import React, { useCallback, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { LanInterfaceView } from '@/api/tauri-command/mobile_sync'
import { Button } from '@/components/ui/button'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { toast } from '@/components/ui/toast'

interface BaseUrlChipProps {
  baseUrl: string
  interfaces: LanInterfaceView[]
  port: string
  selectedHost: string | null
  onSelect: (host: string) => void
}

export const BaseUrlChip: React.FC<BaseUrlChipProps> = ({
  baseUrl,
  interfaces,
  port,
  selectedHost,
  onSelect,
}) => {
  const { t } = useTranslation()
  // 阈值 > 0:只要有候选就走 dropdown。即使只有 1 项, UI 一致性优先,
  // 让用户能"打开看清这是仅有的候选"——而不是看不到下拉箭头怀疑是不
  // 是控件还没初始化好。
  const hasOptions = interfaces.length > 0 && port !== ''

  return (
    <div className="flex max-w-full items-center gap-1 rounded-md border border-border/60 bg-card px-2 py-1">
      {hasOptions ? (
        <Select
          value={selectedHost ?? ''}
          onValueChange={onSelect}
          aria-label={t('devices.mobileSync.credential.baseUrl.selectAria')}
        >
          <SelectTrigger
            size="sm"
            className="h-7 min-w-0 gap-1 rounded-none border-0 bg-transparent px-0 py-0 font-mono shadow-none focus-visible:ring-0 focus-visible:ring-offset-0"
            aria-label={t('devices.mobileSync.credential.baseUrl.selectAria')}
          >
            <SelectValue>
              <span className="truncate font-mono text-sm">{baseUrl}</span>
            </SelectValue>
          </SelectTrigger>
          <SelectContent>
            {interfaces.map(iface => {
              const url = `http://${iface.ipv4}:${port}`
              return (
                <SelectItem key={`${iface.name}-${iface.ipv4}`} value={iface.ipv4}>
                  <div className="flex flex-col items-start gap-0.5">
                    <span className="font-mono text-sm">{url}</span>
                    <span className="text-xs text-muted-foreground">{iface.name}</span>
                  </div>
                </SelectItem>
              )
            })}
          </SelectContent>
        </Select>
      ) : (
        <span className="truncate font-mono text-sm">{baseUrl}</span>
      )}
      <CopyIconButton value={baseUrl} />
    </div>
  )
}

export const CopyIconButton: React.FC<{ value: string }> = ({ value }) => {
  const { t } = useTranslation()
  const [copied, setCopied] = useState(false)
  const handleCopy = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(value)
      setCopied(true)
      setTimeout(() => setCopied(false), 1500)
    } catch {
      toast.error('Copy failed')
    }
  }, [value])

  const label = copied
    ? t('devices.mobileSync.credential.copied')
    : t('devices.mobileSync.credential.copy')

  return (
    <Button
      type="button"
      size="icon-sm"
      variant="ghost"
      aria-label={label}
      title={label}
      onClick={handleCopy}
    >
      {copied ? (
        <Check className="h-3.5 w-3.5 text-emerald-500" />
      ) : (
        <Copy className="h-3.5 w-3.5" />
      )}
    </Button>
  )
}
