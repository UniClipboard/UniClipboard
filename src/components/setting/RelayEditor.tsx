import { Loader2, Save, SignalHigh, Trash2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Button, Input } from '@/components/ui'
import { ProbeResult } from './ProbeResult'
import { RelayCredentialFields } from './RelayCredentialFields'
import { useRelayEditor, type RelayEditorOptions } from './useRelayEditor'
interface RelayEditorProps extends RelayEditorOptions {
  index: number
  removable: boolean
}

export function RelayEditor({ index, removable, ...options }: RelayEditorProps) {
  const { t } = useTranslation()
  const { initialUrl } = options
  const displayIndex = index + 1
  const editor = useRelayEditor(options)
  const {
    url,
    probeStatus,
    saving,
    error,
    canTest,
    canSave,
    updateUrl,
    testAvailability,
    saveRelay,
    removeRelay,
  } = editor
  return (
    <section
      className="rounded-lg border border-border/60 bg-card p-4"
      aria-labelledby={`relay-node-${displayIndex}-title`}
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h5 id={`relay-node-${displayIndex}-title`} className="text-sm font-medium">
          {t('settings.sections.network.customRelays.rowLabel', { index: displayIndex })}
        </h5>
        <Button
          type="button"
          variant="ghost"
          size="icon-sm"
          className="text-muted-foreground hover:text-destructive"
          aria-label={t('settings.sections.network.customRelays.removeAriaLabel', {
            index: displayIndex,
          })}
          disabled={!removable || saving}
          onClick={() => void removeRelay()}
        >
          <Trash2 aria-hidden="true" />
        </Button>
      </div>

      <div className="mt-3 space-y-1.5">
        <label htmlFor={`custom-relay-url-${displayIndex}`} className="text-xs font-medium">
          {t('settings.sections.network.customRelays.urlLabel')}
        </label>
        <Input
          id={`custom-relay-url-${displayIndex}`}
          type="url"
          inputMode="url"
          autoComplete="off"
          value={url}
          placeholder={t('settings.sections.network.customRelays.placeholder')}
          aria-label={t('settings.sections.network.customRelays.itemAriaLabel', {
            index: displayIndex,
          })}
          className="h-9 border-border/60 bg-muted/20 font-mono text-xs shadow-none"
          disabled={saving}
          onChange={event => updateUrl(event.target.value)}
        />
      </div>

      <RelayCredentialFields displayIndex={displayIndex} initialUrl={initialUrl} {...editor} />

      <div className="mt-4 border-t border-border/40 pt-3">
        <p className="text-xs leading-relaxed text-muted-foreground">
          {t('settings.sections.network.customRelays.testHint')}
        </p>
        <div className="mt-3 flex flex-wrap items-center justify-between gap-2">
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={!canTest}
            onClick={() => void testAvailability()}
          >
            {probeStatus.kind === 'testing' ? (
              <Loader2 aria-hidden="true" className="animate-spin" />
            ) : (
              <SignalHigh aria-hidden="true" />
            )}
            {t('settings.sections.network.customRelays.testButton')}
          </Button>
          <Button type="button" size="sm" disabled={!canSave} onClick={() => void saveRelay()}>
            {saving ? (
              <Loader2 aria-hidden="true" className="animate-spin" />
            ) : (
              <Save aria-hidden="true" />
            )}
            {t('settings.sections.network.customRelays.saveButton')}
          </Button>
        </div>
      </div>
      <ProbeResult status={probeStatus} />

      {error && (
        <p className="mt-2 text-xs text-destructive" role="alert">
          {error}
        </p>
      )}
    </section>
  )
}
