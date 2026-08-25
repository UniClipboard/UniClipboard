import { Check, Plus } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import StatusDot, { type StatusDotTone } from '@/components/device/StatusDot'
import AddSpaceDialog from '@/components/spaces/AddSpaceDialog'
import { Button } from '@/components/ui/button'
import { useAppDispatch, useAppSelector } from '@/store/hooks'
import { fetchSpaces, selectActiveSendSpace } from '@/store/spacesSlice'

const runtimeTone: Record<string, StatusDotTone> = {
  stopped: 'off',
  starting: 'info',
  running: 'success',
  locked: 'warning',
  failed: 'warning',
}

export default function SpaceSelector() {
  const { t } = useTranslation()
  const dispatch = useAppDispatch()
  const [addSpaceOpen, setAddSpaceOpen] = useState(false)
  const {
    items,
    listLoading,
    listError,
    mutationError,
    activeSendError,
    activeSendPendingProfileId,
  } = useAppSelector(state => state.spaces)

  useEffect(() => {
    void dispatch(fetchSpaces())
  }, [dispatch])

  return (
    <section
      aria-label={t('spaces.selector.ariaLabel')}
      className="flex shrink-0 items-center gap-2 border-b border-border bg-card px-3 py-2 text-card-foreground"
    >
      <ul className="flex min-w-0 flex-1 gap-2 overflow-x-auto">
        {items.map(space => {
          const name = space.displayName?.trim() || space.deviceName?.trim() || space.profileId
          const selecting = activeSendPendingProfileId === space.profileId
          const faultText = space.lastFault
            ? t(
                space.lastFault.messageCode === 'relay_unreachable'
                  ? 'spaces.faults.relayUnreachable'
                  : 'spaces.faults.generic'
              )
            : null
          const runtimeLabel = t(`spaces.runtime.${space.runtimeState.state}`)
          const incomingLabel = t(`spaces.incoming.${space.incomingSyncState.state}`)

          return (
            <li
              aria-label={name}
              key={space.profileId}
              className="min-w-40 rounded-lg border border-border bg-background p-2"
            >
              <button
                type="button"
                aria-label={
                  space.isActiveSend
                    ? t('spaces.status.active', { name })
                    : t('spaces.status.setActive', { name })
                }
                aria-pressed={space.isActiveSend}
                aria-busy={selecting}
                disabled={selecting}
                onClick={() => void dispatch(selectActiveSendSpace(space.profileId))}
                className="flex w-full items-center gap-2 rounded-md text-left outline-none focus-visible:ring-3 focus-visible:ring-ring/50"
              >
                <StatusDot tone={runtimeTone[space.runtimeState.state] ?? 'off'} />
                <span className="min-w-0 flex-1 truncate text-sm font-medium">{name}</span>
                {space.isActiveSend ? <Check aria-hidden className="size-4 text-primary" /> : null}
              </button>
              <div className="mt-1 flex flex-wrap gap-x-2 text-xs text-muted-foreground">
                <span>{t('spaces.status.runtime', { state: runtimeLabel })}</span>
                <span>{t('spaces.status.incoming', { state: incomingLabel })}</span>
              </div>
              {faultText ? (
                <p className="mt-1 text-xs font-medium text-destructive">{faultText}</p>
              ) : null}
            </li>
          )
        })}
        {listLoading && items.length === 0 ? (
          <li className="self-center text-sm text-muted-foreground">
            {t('spaces.selector.loading')}
          </li>
        ) : null}
        {!listLoading && items.length === 0 && !listError ? (
          <li className="self-center text-sm text-muted-foreground">
            {t('spaces.selector.empty')}
          </li>
        ) : null}
      </ul>
      <Button
        type="button"
        variant="outline"
        size="sm"
        aria-label={t('spaces.actions.add')}
        onClick={() => setAddSpaceOpen(true)}
      >
        <Plus aria-hidden />
        {t('spaces.actions.add')}
      </Button>
      {activeSendError || mutationError || listError ? (
        <p role="alert" className="text-xs font-medium text-destructive">
          {t(activeSendError ?? mutationError ?? listError ?? 'spaces.errors.refresh')}
        </p>
      ) : null}
      <AddSpaceDialog open={addSpaceOpen} onOpenChange={setAddSpaceOpen} />
    </section>
  )
}
