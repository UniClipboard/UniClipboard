import { RotateCcw } from 'lucide-react'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { KeyRecorder } from '@/components/setting/KeyRecorder'
import { SettingRow } from '@/components/setting/SettingRow'
import { ShortcutKeys } from '@/components/setting/ShortcutKeys'
import { Button } from '@/components/ui'
import { Popover, PopoverContent, PopoverTitle, PopoverTrigger } from '@/components/ui/popover'
import { toast } from '@/components/ui/toast'
import type { ShortcutKeyOverrides } from '@/shortcuts/conflicts'
import type { ShortcutDefinition } from '@/shortcuts/definitions'

interface ShortcutRowProps {
  definition: ShortcutDefinition
  currentKey: string
  currentOverrides: ShortcutKeyOverrides
  isModified: boolean
  /** Rejects when the save failed; the recorder then stays open and shows the error. */
  onOverrideChange: (id: string, key: string, clearedIds?: string[]) => Promise<void>
  onResetShortcut: (id: string) => Promise<void>
}

export function ShortcutRow({
  definition,
  currentKey,
  currentOverrides,
  isModified,
  onOverrideChange,
  onResetShortcut,
}: ShortcutRowProps) {
  const { t } = useTranslation()
  const [isRecording, setIsRecording] = useState(false)
  const [saving, setSaving] = useState(false)
  const [saveError, setSaveError] = useState<string | null>(null)
  const modifiedLabel = useMemo(
    () =>
      isModified ? (
        <span className="text-ui-caption font-medium text-primary">
          {t('settings.sections.shortcuts.modified')}
        </span>
      ) : undefined,
    [isModified, t]
  )

  const handleConfirm = async (key: string, clearedIds?: string[]) => {
    setSaving(true)
    setSaveError(null)
    try {
      await onOverrideChange(definition.id, key, clearedIds)
      setIsRecording(false)
    } catch (err) {
      // A refused global shortcut (taken by another program, reserved by the system, not
      // supported by this platform) was not saved; say so instead of closing silently.
      setSaveError(saveErrorMessage(err))
    } finally {
      setSaving(false)
    }
  }

  const setRecording = (open: boolean) => {
    setIsRecording(open)
    if (!open) setSaveError(null)
  }

  const handleCancel = () => {
    setRecording(false)
  }

  const handleReset = () => {
    onResetShortcut(definition.id).catch((err: unknown) =>
      toast.error(t('settings.sections.shortcuts.saveFailed', { message: saveErrorMessage(err) }))
    )
  }

  return (
    <SettingRow label={t(definition.description)} labelExtra={modifiedLabel}>
      <div className="flex items-center gap-2 shrink-0">
        <Popover open={isRecording} onOpenChange={setRecording}>
          <PopoverTrigger
            aria-label={`${t('settings.sections.shortcuts.edit')} ${t(definition.description)}`}
            className="inline-flex min-h-8 min-w-20 max-w-52 items-center justify-center rounded-lg border border-border bg-muted/30 px-2.5 py-1 text-ui-body transition-colors hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40 data-popup-open:border-ring data-popup-open:bg-muted"
          >
            {currentKey ? (
              <ShortcutKeys shortcut={currentKey} />
            ) : (
              <span className="text-muted-foreground">{t('settings.sections.shortcuts.edit')}</span>
            )}
          </PopoverTrigger>
          <PopoverContent
            align="end"
            finalFocus
            className="w-80 max-w-[calc(100vw-2rem)] gap-3 bg-card p-4 text-card-foreground"
          >
            <PopoverTitle className="text-ui-body font-medium">
              {t(definition.description)}
            </PopoverTitle>
            {isRecording && (
              <KeyRecorder
                shortcutId={definition.id}
                scope={definition.scope}
                currentOverrides={currentOverrides}
                onConfirm={(key, clearedIds) => void handleConfirm(key, clearedIds)}
                onCancel={handleCancel}
                saveError={saveError}
                saving={saving}
              />
            )}
          </PopoverContent>
        </Popover>

        {isModified && (
          <Button
            variant="ghost"
            size="icon"
            className="size-7"
            onClick={handleReset}
            title={t('settings.sections.shortcuts.reset')}
          >
            <RotateCcw className="size-3.5" />
          </Button>
        )}
      </div>
    </SettingRow>
  )
}

/** The message of a rejected save: a host command error carries `{ code, message }`. */
export function saveErrorMessage(err: unknown): string {
  if (err && typeof err === 'object' && 'message' in err && typeof err.message === 'string') {
    return err.message
  }
  return String(err)
}
