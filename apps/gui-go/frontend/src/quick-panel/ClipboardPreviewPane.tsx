import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import ClipboardPreview from '@/components/clipboard/ClipboardPreview'
import { usePlatform } from '@/hooks/usePlatform'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'
import {
  getQuickPanelLayoutClassNames,
  QUICK_PANEL_FOOTER_CLASS_NAME,
} from '@/quick-panel/constants'

interface ClipboardPreviewPaneProps {
  item: DisplayClipboardItem | null
}

function ClipboardPreviewPane({ item }: ClipboardPreviewPaneProps) {
  const { t } = useTranslation(undefined, { keyPrefix: 'previewPanel' })
  const isMac = useMemo(() => navigator.platform.toUpperCase().includes('MAC'), [])
  const { isLinux, isTauri } = usePlatform()

  return (
    <div className={getQuickPanelLayoutClassNames(isLinux && isTauri).previewCard}>
      <div className="min-h-0 flex-1" data-testid="quick-panel-preview-area">
        <ClipboardPreview item={item} />
      </div>

      <div className={QUICK_PANEL_FOOTER_CLASS_NAME}>
        <span>{t('deleteHint', { modifier: isMac ? '⌥' : 'Alt+' })}</span>
      </div>
    </div>
  )
}

export default ClipboardPreviewPane
