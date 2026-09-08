import { RotateCcw } from 'lucide-react'
import { useState } from 'react'
import { HexColorPicker } from 'react-colorful'
import { useTranslation } from 'react-i18next'
import { Button, Popover, PopoverContent, PopoverTrigger } from '@/components/ui'
import { hexToOklch, oklchToHexSafe } from '@/lib/color-convert'
import { createLogger } from '@/lib/logger'

const log = createLogger('appearance-color-picker')
export default function AppearanceColorPicker({
  label,
  presetColor,
  overrideColor,
  onChange,
}: {
  label: string
  presetColor: string
  overrideColor: string | null
  onChange: (value: string | null) => Promise<void>
}) {
  const { t } = useTranslation()
  const hex = oklchToHexSafe(overrideColor ?? presetColor)
  const [open, setOpen] = useState(false)
  const [draft, setDraft] = useState(hex)
  const [failed, setFailed] = useState(false)
  const save = async (value: string | null) => {
    setFailed(false)
    try {
      await onChange(value === null ? null : hexToOklch(value))
    } catch (error) {
      log.error({ err: error }, 'Failed to save custom color')
      setFailed(true)
    }
  }
  const change = (value: string) => {
    setDraft(value)
    if (/^#(?:[0-9a-fA-F]{3}|[0-9a-fA-F]{6})$/.test(value)) void save(value)
  }
  const resetLabel = `${label}: ${t('settings.sections.appearance.tokenPicker.reset')}`
  return (
    <div className="py-2">
      <div className="flex min-w-0 items-center justify-between gap-2 text-xs">
        <span>{label}</span>
        <div className="flex shrink-0 items-center gap-1">
          <Popover
            open={open}
            onOpenChange={next => {
              if (next) setDraft(hex)
              setOpen(next)
            }}
          >
            <PopoverTrigger
              render={
                <button
                  type="button"
                  aria-label={label}
                  className="flex items-center gap-2 rounded-sm px-1.5 py-1 outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring"
                />
              }
            >
              <span
                aria-hidden="true"
                className="size-4 rounded-sm border border-border"
                style={{ backgroundColor: hex }}
              />
              <span className="w-14 text-left font-mono text-[0.6875rem] uppercase text-muted-foreground">
                {hex}
              </span>
            </PopoverTrigger>
            <PopoverContent align="end" className="w-auto max-w-[calc(100vw-2rem)] p-3">
              <HexColorPicker
                color={/^#[0-9a-fA-F]{6}$/.test(draft) ? draft : hex}
                onChange={change}
              />
              <input
                aria-label={`${label}: ${t('settings.sections.appearance.tokenPicker.hexInputLabel')}`}
                value={open ? draft : hex}
                onChange={event => change(event.target.value.trim())}
                spellCheck={false}
                className="mt-3 w-full rounded-sm border border-border bg-background px-2 py-1.5 font-mono text-xs"
              />
            </PopoverContent>
          </Popover>
          <Button
            variant="ghost"
            size="icon-xs"
            title={resetLabel}
            aria-label={resetLabel}
            disabled={overrideColor === null}
            onClick={() => {
              setOpen(false)
              void save(null)
            }}
          >
            <RotateCcw />
          </Button>
        </div>
      </div>
      {failed && (
        <p role="alert" className="mt-1 text-xs text-destructive">
          {t('appearanceLayout.saveFailed')}
        </p>
      )}
    </div>
  )
}
