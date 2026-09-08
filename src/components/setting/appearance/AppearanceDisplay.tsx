import { Minus, Plus, RotateCcw } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  Button,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
  Switch,
} from '@/components/ui'
import { useUiScale } from '@/hooks/useUiScale'
import { useWindowFrame } from '@/hooks/useWindowFrame'
import { createLogger } from '@/lib/logger'

const log = createLogger('appearance-display')
export default function AppearanceDisplay() {
  const { t } = useTranslation()
  const zoom = useUiScale()
  const frame = useWindowFrame()
  const [saving, setSaving] = useState(false)
  const [failed, setFailed] = useState(false)
  const toggleFrame = async (enabled: boolean) => {
    setSaving(true)
    setFailed(false)
    try {
      await frame.setUseSystemWindowFrame(enabled)
    } catch (error) {
      log.error({ err: error }, 'Failed to change window frame')
      setFailed(true)
    } finally {
      setSaving(false)
    }
  }
  return (
    <div className="divide-y divide-border/25">
      <div className="appearance-row py-4">
        <div>
          <span className="text-sm font-normal">
            {t('settings.sections.appearance.zoom.title')}
          </span>
          <p className="mt-1 text-xs leading-relaxed text-muted-foreground">
            {t('appearanceLayout.scaleHelp')}
          </p>
        </div>
        <div className="grid min-w-0 grid-cols-[2.25rem_minmax(0,1fr)_2.25rem_2.25rem] items-center gap-3">
          <Button
            variant="outline"
            size="icon-sm"
            className="size-9"
            aria-label={t('appearanceLayout.zoomOut')}
            title={t('appearanceLayout.zoomOut')}
            disabled={!zoom.canZoomOut}
            onClick={zoom.zoomOut}
          >
            <Minus />
          </Button>
          <Select value={String(zoom.scale)} onValueChange={value => zoom.setScale(Number(value))}>
            <SelectTrigger
              aria-label={t('settings.sections.appearance.zoom.title')}
              className="h-9 w-full min-w-0 text-sm"
            >
              <SelectValue>{zoom.scalePercent}</SelectValue>
            </SelectTrigger>
            <SelectContent>
              {zoom.options.map(option => (
                <SelectItem key={option.value} value={String(option.value)}>
                  {option.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Button
            variant="outline"
            size="icon-sm"
            className="size-9"
            aria-label={t('appearanceLayout.zoomIn')}
            title={t('appearanceLayout.zoomIn')}
            disabled={!zoom.canZoomIn}
            onClick={zoom.zoomIn}
          >
            <Plus />
          </Button>
          <Button
            variant="outline"
            size="icon-sm"
            className="size-9"
            aria-label={t('settings.sections.appearance.zoom.reset')}
            title={t('settings.sections.appearance.zoom.reset')}
            disabled={zoom.isDefault}
            onClick={zoom.resetScale}
          >
            <RotateCcw />
          </Button>
        </div>
      </div>
      {frame.canChooseSystemFrame && (
        <div className="appearance-row py-4">
          <div className="min-w-0">
            <label htmlFor="appearance-system-frame" className="text-sm font-normal">
              {t('settings.sections.appearance.windowFrame.useSystem')}
            </label>
            <p className="mt-1 max-w-sm text-xs leading-relaxed text-muted-foreground">
              {t('settings.sections.appearance.windowFrame.description')}
            </p>
            {failed && (
              <p role="alert" className="mt-1 text-xs text-destructive">
                {t('appearanceLayout.saveFailed')}
              </p>
            )}
          </div>
          <Switch
            id="appearance-system-frame"
            aria-label={t('settings.sections.appearance.windowFrame.useSystem')}
            checked={frame.useSystemWindowFrame}
            disabled={saving}
            onCheckedChange={value => {
              void toggleFrame(value)
            }}
          />
        </div>
      )}
    </div>
  )
}
