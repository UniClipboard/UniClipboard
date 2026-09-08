import { Info } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip'
import { useVisualEffects, useVisualEffectsUnavailable } from '@/hooks/useVisualEffects'
import type { EffectsMode } from '@/lib/ipc-bindings.generated'
import { visualEffectsStore } from '@/lib/visual-effects-store'
import '@/components/setting/appearance/appearance-layout.css'

const MODES: EffectsMode[] = ['auto', 'effects', 'smooth']

export default function SmoothModeSetting() {
  const { t } = useTranslation()
  const effects = useVisualEffects()
  const unavailable = useVisualEffectsUnavailable()
  const [saving, setSaving] = useState(false)
  const setMode = async (mode: EffectsMode) => {
    setSaving(true)
    try {
      await visualEffectsStore.setMode(mode)
    } catch {
      /* The shared status displays the failure. */
    } finally {
      setSaving(false)
    }
  }
  return (
    <fieldset className="appearance-row border-0" disabled={saving || !effects.sessionId}>
      <legend className="sr-only">{t('smoothMode.title')}</legend>
      <div className="appearance-label-block">
        <span aria-hidden="true" className="text-sm font-normal">
          {t('smoothMode.title')}
        </span>
        <p className="mt-1 text-xs leading-relaxed text-muted-foreground">
          {t('appearanceLayout.effectsHelp')}
        </p>
      </div>
      <div className="grid grid-cols-3 gap-1 rounded-md bg-muted p-1">
        {MODES.map(mode => (
          <label key={mode} className="relative min-w-0 cursor-pointer">
            <input
              type="radio"
              name="smooth-mode"
              value={mode}
              checked={effects.mode === mode}
              onChange={() => {
                void setMode(mode)
              }}
              className="peer sr-only"
            />
            <span className="flex h-full min-h-7 items-center justify-center rounded-sm px-2 py-1 text-center text-sm break-words peer-checked:bg-background peer-checked:font-medium peer-focus-visible:outline-2 peer-focus-visible:outline-ring peer-disabled:opacity-60">
              {t(`smoothMode.${mode}`)}
            </span>
          </label>
        ))}
      </div>
      <div className="appearance-note flex items-center gap-1.5 text-xs leading-relaxed text-muted-foreground">
        <p role="status">
          {t(effects.lowEffects ? 'smoothMode.currentSmooth' : 'smoothMode.currentEffects')}
        </p>
        <TooltipProvider>
          <Tooltip>
            <TooltipTrigger
              render={
                <button
                  type="button"
                  aria-label={t(`smoothMode.reason.${effects.reason}`)}
                  className="order-first -ml-1 flex size-5 shrink-0 items-center justify-center rounded-sm outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
                />
              }
            >
              <Info aria-hidden="true" className="size-3.5" />
            </TooltipTrigger>
            <TooltipContent role="tooltip" className="max-w-xs">
              {t(`smoothMode.reason.${effects.reason}`)}
            </TooltipContent>
          </Tooltip>
        </TooltipProvider>
      </div>
      {(unavailable || effects.persistence === 'session_only') && (
        <p role="alert" className="appearance-note text-xs text-destructive">
          {t(unavailable ? 'smoothMode.unavailable' : 'smoothMode.notSaved')}
        </p>
      )}
    </fieldset>
  )
}
