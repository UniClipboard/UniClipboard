import { Plus } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { RelayEditor } from '@/components/setting/RelayEditor'
import { SettingGroup } from '@/components/setting/SettingGroup'
import { Button, TooltipProvider } from '@/components/ui'
import type { CustomRelay, CustomRelayMutation, CustomRelayMutationResult } from '@/types/setting'

interface CustomRelayUrlsFieldProps {
  value: CustomRelay[]
  onSave: (mutation: CustomRelayMutation) => Promise<CustomRelayMutationResult>
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

  const commitMutation = async (
    draftRowId: string | null,
    mutation: CustomRelayMutation
  ): Promise<CustomRelayMutationResult> => {
    const result = await onSave(mutation)
    if (draftRowId) discardDraft(draftRowId)
    return result
  }

  return (
    <TooltipProvider delay={200}>
      <SettingGroup title={t('settings.sections.network.groups.relays')}>
        <div className="px-1 pt-3">
          <p className="max-w-prose text-ui-caption-relaxed text-muted-foreground">
            {t('settings.sections.network.customRelays.description')}
          </p>
          <div className="mt-4 flex flex-col gap-4">
            {value.map((relay, index) => (
              <RelayEditor
                key={relay.url}
                index={index}
                initialUrl={relay.url}
                initialCredentialConfigured={relay.credentialConfigured}
                onRemove={async () => {
                  await onSave({
                    action: 'delete',
                    url: relay.url,
                  })
                }}
                onSave={(nextUrl, credential) =>
                  commitMutation(null, {
                    action: 'edit',
                    previousUrl: relay.url,
                    url: nextUrl,
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
                initialCredentialConfigured={false}
                onRemove={() => discardDraft(draftRowId)}
                onSave={(nextUrl, credential) =>
                  commitMutation(draftRowId, {
                    action: 'add',
                    url: nextUrl,
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
      </SettingGroup>
    </TooltipProvider>
  )
}
