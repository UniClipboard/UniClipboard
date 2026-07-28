import { Plus } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Button, TooltipProvider } from '@/components/ui'
import type { RelaySaveContextResult, RelaySaveMutation } from '@/types/setting'
import { RelayEditor } from './RelayEditor'

interface CustomRelayUrlsFieldProps {
  value: string[]
  onSave: (mutation: RelaySaveMutation) => Promise<RelaySaveContextResult>
}

let nextDraftRowId = 0

function allocateDraftRowId(): string {
  nextDraftRowId += 1
  return `new-relay-${nextDraftRowId}`
}

export function CustomRelayUrlsField({ value, onSave }: CustomRelayUrlsFieldProps) {
  const { t } = useTranslation()
  const [draftRowIds, setDraftRowIds] = useState<string[]>(() =>
    value.length === 0 ? [allocateDraftRowId()] : []
  )
  const canAddRelay = draftRowIds.length === 0

  const addRelay = () => {
    if (!canAddRelay) return
    setDraftRowIds([allocateDraftRowId()])
  }

  const discardDraft = (draftRowId: string) => {
    setDraftRowIds(previous => previous.filter(id => id !== draftRowId))
  }

  const saveRelay = async (
    draftRowId: string | null,
    mutation: RelaySaveMutation
  ): Promise<RelaySaveContextResult> => {
    const result = await onSave(mutation)
    if (draftRowId) discardDraft(draftRowId)
    return result
  }

  const occurrenceCounts = new Map<string, number>()
  const savedRows = value.map((url, index) => {
    const occurrence = occurrenceCounts.get(url) ?? 0
    occurrenceCounts.set(url, occurrence + 1)
    return { index, key: `${url}\u0000${occurrence}`, url }
  })

  return (
    <TooltipProvider delay={200}>
      <div className="px-4 py-3">
        <div className="space-y-0.5">
          <h4 className="text-sm font-medium">
            {t('settings.sections.network.customRelays.label')}
          </h4>
          <p className="text-xs leading-snug text-muted-foreground">
            {t('settings.sections.network.customRelays.description')}
          </p>
        </div>

        <div className="mt-3 divide-y divide-border/50 border-y border-border/50">
          {savedRows.map(({ index, key, url }) => (
            <RelayEditor
              key={key}
              index={index}
              initialUrl={url}
              onRemove={async () => {
                await onSave({
                  index,
                  previousUrl: url,
                  nextUrl: null,
                  credential: { action: 'delete' },
                })
              }}
              onSave={(nextUrl, credential) =>
                saveRelay(null, {
                  index,
                  previousUrl: url,
                  nextUrl,
                  credential,
                })
              }
              removable
            />
          ))}
          {draftRowIds.map((draftRowId, draftIndex) => (
            <RelayEditor
              key={draftRowId}
              index={value.length + draftIndex}
              initialUrl=""
              onRemove={() => discardDraft(draftRowId)}
              onSave={(nextUrl, credential) =>
                saveRelay(draftRowId, {
                  index: null,
                  previousUrl: null,
                  nextUrl,
                  credential,
                })
              }
              removable={value.length > 0 || draftRowIds.length > 1}
            />
          ))}
        </div>

        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="mt-2 h-8 px-2 text-muted-foreground hover:text-foreground"
          disabled={!canAddRelay}
          onClick={addRelay}
        >
          <Plus aria-hidden="true" />
          {t('settings.sections.network.customRelays.addButton')}
        </Button>
      </div>
    </TooltipProvider>
  )
}
