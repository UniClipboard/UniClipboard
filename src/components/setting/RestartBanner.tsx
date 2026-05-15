import { RefreshCw, X } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'

/**
 * RestartBanner — NetworkSection 专属持久 inline 重启通知 (Phase 95)。
 *
 * - Per CONTEXT D-A1: 不复用 shadcn Alert, 不用 sonner toast。
 * - Per CONTEXT D-A2: 由父组件 (NetworkSection) 嵌入到 SettingGroup 内部、Switch 上方。
 * - Per CONTEXT D-A3: 三态视觉只靠 Banner 出现/消失表达, Switch 自身样式不动。
 * - Per CONTEXT D-B3: app.restart() 失败时通过 error prop 渲染 inline error, 不抛 toast。
 *
 * Scope: 这个 banner 跟 NetworkSection 的 loading/error/retry 状态机紧绑,
 * i18n key 走 `settings.sections.network.restartBanner.*`。其他 surface
 * (例如 MobileSyncSettingsDialog) 的"提示重启"需求视觉与错误处理都不同,
 * 应该各自走自己的 inline UI, 不要尝试把这个 banner 改造成 generic ——
 * union 类型的 banner 会比复制一份更难维护。
 */
export interface RestartBannerProps {
  visible: boolean
  onRestart: () => Promise<void>
  loading?: boolean
  error?: string | null
  onDismissError?: () => void
}

/**
 * Renders a restart notification banner with actions for restarting or retrying a network-related operation.
 *
 * When `visible` is false the component renders nothing. When visible it announces status to assistive
 * technologies, shows a translated message, optionally displays an inline error, and presents action
 * buttons whose labels and availability are driven by `loading` and `error`.
 *
 * @param visible - Controls whether the banner is rendered.
 * @param onRestart - Invoked when the user activates the restart/retry action.
 * @param loading - When true, disables action buttons and switches the primary action label to a "restarting" state.
 * @param error - If provided, displays the error text inline and switches actions to "retry" plus a dismiss button.
 * @param onDismissError - Called when the dismiss (icon) button is clicked to clear or acknowledge the error.
 * @returns The banner's React element when visible, or `null` when not visible.
 */
export function RestartBanner({
  visible,
  onRestart,
  loading = false,
  error = null,
  onDismissError,
}: RestartBannerProps) {
  const { t } = useTranslation()

  if (!visible) return null

  return (
    <div
      role="status"
      aria-live="polite"
      className="flex items-start gap-2 px-4 py-3 bg-accent/40 border-b border-border/40 rounded-none"
    >
      <RefreshCw className="size-4 text-foreground mt-0.5 shrink-0" aria-hidden="true" />
      <div className="flex-1 space-y-1">
        <p className="text-sm text-foreground">
          {t('settings.sections.network.restartBanner.message')}
        </p>
        {error && (
          <p role="alert" className="text-xs text-destructive">
            {error}
          </p>
        )}
      </div>
      <div className="ml-auto flex items-center gap-2">
        {!error ? (
          <Button
            variant="default"
            size="sm"
            onClick={() => {
              void onRestart()
            }}
            disabled={loading}
          >
            {loading
              ? t('settings.sections.network.restartBanner.restartingButton')
              : t('settings.sections.network.restartBanner.restartButton')}
          </Button>
        ) : (
          <>
            <Button
              variant="outline"
              size="sm"
              onClick={() => {
                void onRestart()
              }}
              disabled={loading}
            >
              {t('settings.sections.network.restartBanner.retryButton')}
            </Button>
            {onDismissError && (
              <Button
                variant="ghost"
                size="icon"
                aria-label={t('settings.sections.network.restartBanner.dismissAriaLabel')}
                onClick={onDismissError}
              >
                <X className="size-3.5" aria-hidden="true" />
              </Button>
            )}
          </>
        )}
      </div>
    </div>
  )
}

export default RestartBanner
