import { Info } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Popover, PopoverTrigger, PopoverContent } from '@/components/ui/popover'

/**
 * AllowOverlayAddrsDisclosure — info icon click-only Popover, 解释什么是虚拟网络
 * 地址、为什么默认关闭、什么时候应该开启。
 *
 * 与 LanOnlyDisclosure 同模式：click-only（hover 容纳量不够 + 内容不可复制）。
 */
const DISCLOSURE_KEYS = ['covered', 'whyOff', 'whenOn', 'tradeoff'] as const

export function AllowOverlayAddrsDisclosure() {
  const { t } = useTranslation()

  return (
    <Popover>
      <PopoverTrigger asChild>
        <button
          type="button"
          aria-label={t('settings.sections.network.allowOverlayAddrs.infoIconAriaLabel')}
          aria-haspopup="dialog"
          className="inline-flex items-center justify-center rounded-md p-1 text-muted-foreground hover:text-foreground hover:bg-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
        >
          <Info className="size-3.5" aria-hidden="true" />
        </button>
      </PopoverTrigger>
      <PopoverContent
        align="start"
        sideOffset={8}
        aria-labelledby="allow-overlay-addrs-disclosure-title"
      >
        <div className="space-y-3">
          <div>
            <p id="allow-overlay-addrs-disclosure-title" className="text-sm font-medium">
              {t('settings.sections.network.allowOverlayAddrs.disclosure.title')}
            </p>
            <p className="text-xs text-muted-foreground mt-1">
              {t('settings.sections.network.allowOverlayAddrs.disclosure.intro')}
            </p>
          </div>
          <div className="space-y-2">
            {DISCLOSURE_KEYS.map(key => (
              <div key={key} className="space-y-1">
                <p className="text-sm font-medium">
                  {t(`settings.sections.network.allowOverlayAddrs.disclosure.${key}.title`)}
                </p>
                <p className="text-xs text-muted-foreground leading-snug">
                  {t(`settings.sections.network.allowOverlayAddrs.disclosure.${key}.description`)}
                </p>
              </div>
            ))}
          </div>
        </div>
      </PopoverContent>
    </Popover>
  )
}

export default AllowOverlayAddrsDisclosure
