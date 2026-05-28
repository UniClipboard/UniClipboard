import { RefreshCw, X } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'

/**
 * RestartBanner — 设置页持久 inline 重启通知。
 *
 * 由父 section 把"需要重启的原因文案"作为 `message` 传入,banner 内部统一
 * 管理按钮 / 错误 / dismiss 的视觉与文案(走 `settings.restartBanner.*`
 * 命名空间)。这样新增需要重启提示的 section(如 QuickPanelSection)只用
 * 注入自己的解释文案 + 处理 `onRestart`,不必各自复制一份相同样式的 inline UI。
 *
 * - Per CONTEXT D-A1: 不复用 shadcn Alert, 不用 sonner toast。
 * - Per CONTEXT D-A2: 由父组件嵌入到 SettingGroup 内部、Switch 上方。
 * - Per CONTEXT D-A3: 三态视觉只靠 Banner 出现/消失表达, Switch 自身样式不动。
 * - Per CONTEXT D-B3: `onRestart` 失败时通过 `error` prop 渲染 inline error, 不抛 toast。
 */
export interface RestartBannerProps {
  visible: boolean
  /** 已 i18n 好的"为什么需要重启"说明,由调用方根据自己 section 的语义提供。 */
  message: string
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
  message,
  onRestart,
  loading = false,
  error = null,
  onDismissError,
}: RestartBannerProps) {
  const { t } = useTranslation()

  if (!visible) return null

  return (
    <output
      aria-live="polite"
      className="flex items-start gap-2 px-4 py-3 bg-accent/40 border-b border-border/40 rounded-none"
    >
      <RefreshCw className="size-4 text-foreground mt-0.5 shrink-0" aria-hidden="true" />
      <div className="flex-1 space-y-1">
        <p className="text-sm text-foreground">{message}</p>
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
              ? t('settings.restartBanner.restartingButton')
              : t('settings.restartBanner.restartButton')}
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
              {t('settings.restartBanner.retryButton')}
            </Button>
            {onDismissError && (
              <Button
                variant="ghost"
                size="icon"
                aria-label={t('settings.restartBanner.dismissAriaLabel')}
                onClick={onDismissError}
              >
                <X className="size-3.5" aria-hidden="true" />
              </Button>
            )}
          </>
        )}
      </div>
    </output>
  )
}
