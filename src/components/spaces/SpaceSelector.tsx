import { Check, Plus } from 'lucide-react'
import { useEffect } from 'react'
import StatusDot, { type StatusDotTone } from '@/components/device/StatusDot'
import { Button } from '@/components/ui/button'
import { useAppDispatch, useAppSelector } from '@/store/hooks'
import { fetchSpaces, selectActiveSendSpace } from '@/store/spacesSlice'

interface SpaceSelectorProps {
  onAddSpace: () => void
}

const runtimeTone: Record<string, StatusDotTone> = {
  stopped: 'off',
  starting: 'info',
  running: 'success',
  locked: 'warning',
  failed: 'warning',
}

export default function SpaceSelector({ onAddSpace }: SpaceSelectorProps) {
  const dispatch = useAppDispatch()
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
      aria-label="Space selector"
      className="flex shrink-0 items-center gap-2 border-b border-border bg-card px-3 py-2 text-card-foreground"
    >
      <div role="list" className="flex min-w-0 flex-1 gap-2 overflow-x-auto">
        {items.map(space => {
          const name = space.displayName?.trim() || space.deviceName?.trim() || space.profileId
          const selecting = activeSendPendingProfileId === space.profileId
          const faultText = space.lastFault
            ? `${space.lastFault.category}${space.lastFault.messageCode ? ` (${space.lastFault.messageCode})` : ''}`
            : null

          return (
            <div
              role="listitem"
              aria-label={name}
              key={space.profileId}
              className="min-w-40 rounded-lg border border-border bg-background p-2"
            >
              <button
                type="button"
                aria-label={
                  space.isActiveSend
                    ? `${name}, active send space`
                    : `Set ${name} as active send space`
                }
                aria-pressed={space.isActiveSend}
                disabled={selecting}
                onClick={() => void dispatch(selectActiveSendSpace(space.profileId))}
                className="flex w-full items-center gap-2 rounded-md text-left outline-none focus-visible:ring-3 focus-visible:ring-ring/50"
              >
                <StatusDot tone={runtimeTone[space.runtimeState.state] ?? 'off'} />
                <span className="min-w-0 flex-1 truncate text-sm font-medium">{name}</span>
                {space.isActiveSend ? <Check aria-hidden className="size-4 text-primary" /> : null}
              </button>
              <div className="mt-1 flex flex-wrap gap-x-2 text-xs text-muted-foreground">
                <span>Runtime {space.runtimeState.state}</span>
                <span>Incoming {space.incomingSyncState.state}</span>
              </div>
              {faultText ? (
                <p className="mt-1 text-xs font-medium text-destructive">Fault: {faultText}</p>
              ) : null}
            </div>
          )
        })}
        {listLoading && items.length === 0 ? (
          <p className="self-center text-sm text-muted-foreground">Loading spaces</p>
        ) : null}
        {!listLoading && items.length === 0 && !listError ? (
          <p className="self-center text-sm text-muted-foreground">No spaces available</p>
        ) : null}
      </div>
      <Button type="button" variant="outline" size="sm" aria-label="Add Space" onClick={onAddSpace}>
        <Plus aria-hidden />
        Add Space
      </Button>
      {listError || mutationError || activeSendError ? (
        <p role="alert" className="text-xs font-medium text-destructive">
          {listError ?? mutationError ?? activeSendError}
        </p>
      ) : null}
    </section>
  )
}
