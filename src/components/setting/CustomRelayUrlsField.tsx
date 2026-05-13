import { Plus, Trash2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Button, Input } from '@/components/ui'

interface CustomRelayUrlsFieldProps {
  value: string[]
  onChange: (value: string[]) => void
}

function visibleRows(value: string[]): string[] {
  return value.length > 0 ? value : ['']
}

function collapseSingleEmptyRow(value: string[]): string[] {
  return value.length === 1 && value[0].trim() === '' ? [] : value
}

export function CustomRelayUrlsField({ value, onChange }: CustomRelayUrlsFieldProps) {
  const { t } = useTranslation()
  const rows = visibleRows(value)
  const canRemoveOnlyRow = value.length > 0
  const canAddRow = rows[rows.length - 1]?.trim() !== ''

  const handleRowChange = (index: number, nextValue: string) => {
    const next = [...rows]
    next[index] = nextValue
    onChange(collapseSingleEmptyRow(next))
  }

  const handleRemoveRow = (index: number) => {
    if (!canRemoveOnlyRow && rows.length === 1) return
    onChange(collapseSingleEmptyRow(rows.filter((_, rowIndex) => rowIndex !== index)))
  }

  const handleAddRow = () => {
    if (!canAddRow) return
    onChange([...value, ''])
  }

  return (
    <div className="space-y-3 px-4 py-3">
      <div className="space-y-0.5">
        <label htmlFor="custom-relay-url-0" className="text-sm font-medium">
          {t('settings.sections.network.customRelays.label')}
        </label>
        <p className="text-xs leading-snug text-muted-foreground">
          {t('settings.sections.network.customRelays.description')}
        </p>
      </div>

      <div className="space-y-2">
        {rows.map((url, index) => (
          <div key={index} className="flex min-w-0 items-center gap-2">
            <Input
              id={index === 0 ? 'custom-relay-url-0' : undefined}
              type="url"
              inputMode="url"
              autoComplete="off"
              value={url}
              placeholder={t('settings.sections.network.customRelays.placeholder')}
              aria-label={t('settings.sections.network.customRelays.itemAriaLabel', {
                index: index + 1,
              })}
              className="font-mono text-xs"
              onChange={event => handleRowChange(index, event.target.value)}
            />
            <Button
              type="button"
              variant="ghost"
              size="icon-sm"
              aria-label={t('settings.sections.network.customRelays.removeAriaLabel', {
                index: index + 1,
              })}
              disabled={!canRemoveOnlyRow && rows.length === 1}
              onClick={() => handleRemoveRow(index)}
            >
              <Trash2 aria-hidden="true" />
            </Button>
          </div>
        ))}
      </div>

      <Button
        type="button"
        variant="outline"
        size="sm"
        disabled={!canAddRow}
        onClick={handleAddRow}
      >
        <Plus aria-hidden="true" />
        {t('settings.sections.network.customRelays.addButton')}
      </Button>
    </div>
  )
}
