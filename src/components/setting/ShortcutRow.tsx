import { RotateCcw } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { KeyRecorder } from '@/components/setting/KeyRecorder'
import { ShortcutKeys } from '@/components/setting/ShortcutKeys'
import { Button } from '@/components/ui'
import { Popover, PopoverContent, PopoverTitle, PopoverTrigger } from '@/components/ui/popover'
import type { ShortcutKeyOverrides } from '@/shortcuts/conflicts'
import type { ShortcutDefinition } from '@/shortcuts/definitions'

interface ShortcutRowProps {
  definition: ShortcutDefinition
  currentKey: string
  currentOverrides: ShortcutKeyOverrides
  isModified: boolean
  onOverrideChange: (id: string, key: string, clearedIds?: string[]) => void
  onResetShortcut: (id: string) => void
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

  const handleConfirm = (key: string, clearedIds?: string[]) => {
    onOverrideChange(definition.id, key, clearedIds)
    setIsRecording(false)
  }

  const handleCancel = () => {
    setIsRecording(false)
  }

  const handleReset = () => {
    onResetShortcut(definition.id)
  }

  return (
    <div className="flex items-center justify-between gap-4 px-1 py-2.5">
      <div className="min-w-0 flex-1">
        <span className="text-sm">{t(definition.description)}</span>
        {isModified && (
          <span className="ml-2 text-xs text-primary font-medium">
            {t('settings.sections.shortcuts.modified')}
          </span>
        )}
      </div>

      <div className="flex items-center gap-2 shrink-0">
        <Popover open={isRecording} onOpenChange={setIsRecording}>
          <PopoverTrigger
            aria-label={`${t('settings.sections.shortcuts.edit')} ${t(definition.description)}`}
            className="inline-flex min-h-8 min-w-20 max-w-52 items-center justify-center rounded-lg border border-border bg-muted/30 px-2.5 py-1 text-sm transition-colors hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40 data-popup-open:border-ring data-popup-open:bg-muted"
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
            <PopoverTitle className="text-sm font-medium">{t(definition.description)}</PopoverTitle>
            {isRecording && (
              <KeyRecorder
                shortcutId={definition.id}
                scope={definition.scope}
                currentOverrides={currentOverrides}
                onConfirm={handleConfirm}
                onCancel={handleCancel}
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
    </div>
  )
}
