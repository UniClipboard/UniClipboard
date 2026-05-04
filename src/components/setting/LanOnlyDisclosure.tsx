import { Info } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Popover, PopoverTrigger, PopoverContent } from '@/components/ui/popover'

/**
 * LanOnlyDisclosure — info icon click-only Popover, 列出 LAN-only 模式开启后仍走外网的 4 类请求。
 *
 * - Per CONTEXT D-C1: click-only Popover，禁用 hover 触发（hover 容纳量不够 + 内容不可复制）。
 * - Per CONTEXT D-C2: 4 类外网请求文案最终敲定（Phase 97 反向复制基准）。
 *
 * 文案完全来自 i18n（Plan 03 已 ship 双语，Phase 97 docs 反向复制）。
 * 组件名带 LanOnly 前缀语义清晰，但内部未维护任何 lanOnly 字段（反向命名铁律）。
 */
const DISCLOSURE_KEYS = ['rendezvous', 'otlp', 'pkarr', 'autoUpdate'] as const

export function LanOnlyDisclosure() {
  const { t } = useTranslation()

  return (
    <Popover>
      <PopoverTrigger asChild>
        <button
          type="button"
          aria-label={t('settings.sections.network.lanOnly.infoIconAriaLabel')}
          aria-haspopup="dialog"
          className="inline-flex items-center justify-center rounded-md p-1 text-muted-foreground hover:text-foreground hover:bg-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
        >
          <Info className="size-3.5" aria-hidden="true" />
        </button>
      </PopoverTrigger>
      <PopoverContent align="start" sideOffset={8}>
        <div className="space-y-3">
          <div>
            <p className="text-sm font-medium">
              {t('settings.sections.network.lanOnly.disclosure.title')}
            </p>
            <p className="text-xs text-muted-foreground mt-1">
              {t('settings.sections.network.lanOnly.disclosure.intro')}
            </p>
          </div>
          <div className="space-y-2">
            {DISCLOSURE_KEYS.map(key => (
              <div key={key} className="space-y-1">
                <p className="text-sm font-medium">
                  {t(`settings.sections.network.lanOnly.disclosure.${key}.title`)}
                </p>
                <p className="text-xs text-muted-foreground leading-snug">
                  {t(`settings.sections.network.lanOnly.disclosure.${key}.description`)}
                </p>
              </div>
            ))}
          </div>
        </div>
      </PopoverContent>
    </Popover>
  )
}

export default LanOnlyDisclosure
