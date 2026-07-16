import { useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from '@/components/ui/toast'
import { createLogger } from '@/lib/logger'

const log = createLogger('optimistic-setting')

const DEFAULT_ERROR_KEY = 'settings.sections.general.saveError'

interface OptimisticSettingOptions {
  /** Message logged when the background persist rejects. */
  failureLog?: string
  /** i18n key for the toast shown when the persist rejects. */
  errorKey?: string
}

/**
 * A settings value that updates the UI immediately on change and persists in
 * the background. This is the single source of truth for how an optimistic
 * settings control behaves:
 *
 * - the returned value reflects the user's change instantly (no wait for the
 *   daemon round-trip), so the control never lags behind the click;
 * - persistence runs in the background and the control is NOT disabled while it
 *   runs, so changing one control never dims itself or its siblings;
 * - on failure the value reverts to the last persisted value and a toast fires.
 *
 * Replaces the per-section `form` mirror + `useSavingState` + `isBusy` +
 * `disabled` scaffolding, which coupled "persisting" to "disable/dim the whole
 * group" and produced a flash on every toggle. `disabled` is reserved for
 * genuinely blocking actions, which own their local state instead.
 */
export function useOptimisticSetting<T>(
  current: T,
  persist: (next: T) => Promise<unknown>,
  options?: OptimisticSettingOptions
): [T, (next: T) => void] {
  const { t } = useTranslation()
  const [pending, setPending] = useState<{ id: number; value: T } | null>(null)
  const nextMutationIdRef = useRef(0)
  const value = pending ? pending.value : current

  const set = (next: T) => {
    const id = nextMutationIdRef.current + 1
    nextMutationIdRef.current = id
    setPending({ id, value: next })
    void Promise.resolve(persist(next)).then(
      () => {
        setPending(active => (active?.id === id ? null : active))
      },
      (err: unknown) => {
        log.error({ err }, options?.failureLog ?? 'Failed to persist setting')
        setPending(active => (active?.id === id ? null : active))
        toast.error(t(options?.errorKey ?? DEFAULT_ERROR_KEY))
      }
    )
  }

  return [value, set]
}
