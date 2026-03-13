import { Pencil, RotateCcw } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { KeyRecorder } from '@/components/setting/KeyRecorder'
import { Button } from '@/components/ui'
import { formatKeyPart } from '@/lib/shortcut-format'
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
  const keyParts = currentKey.split('+').map(formatKeyPart)

  const handleEdit = () => {
    setIsRecording(true)
  }

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

  // When recording, show the KeyRecorder component
  if (isRecording) {
    return (
      <div className="px-4 py-2.5">
        <KeyRecorder
          shortcutId={definition.id}
          scope={definition.scope}
          currentOverrides={currentOverrides}
          onConfirm={handleConfirm}
          onCancel={handleCancel}
        />
      </div>
    )
  }

  return (
    <div className="flex items-center justify-between gap-4 px-4 py-2.5">
      <div className="min-w-0 flex-1">
        <span className="text-sm">{t(definition.description)}</span>
        {isModified && (
          <span className="ml-2 text-xs text-primary font-medium">
            {t('settings.sections.shortcuts.modified')}
          </span>
        )}
      </div>

      <div className="flex items-center gap-2 shrink-0">
        <div className="flex items-center gap-0.5">
          {keyParts.map((part, idx) => (
            <span key={idx} className="flex items-center">
              {idx > 0 && <span className="text-muted-foreground text-xs mx-0.5">+</span>}
              <kbd className="bg-muted text-xs font-mono px-1.5 py-0.5 rounded border border-border/60 text-foreground">
                {part}
              </kbd>
            </span>
          ))}
        </div>

        <Button
          variant="ghost"
          size="icon"
          className="h-7 w-7"
          onClick={handleEdit}
          title={t('settings.sections.shortcuts.edit')}
        >
          <Pencil className="h-3.5 w-3.5" />
        </Button>

        {isModified && (
          <Button
            variant="ghost"
            size="icon"
            className="h-7 w-7"
            onClick={handleReset}
            title={t('settings.sections.shortcuts.reset')}
          >
            <RotateCcw className="h-3.5 w-3.5" />
          </Button>
        )}
      </div>
    </div>
  )
}
