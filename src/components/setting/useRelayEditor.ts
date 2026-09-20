import { useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { probeRelayUrl } from '@/api/daemon/settings'
import type { CustomRelayMutationResult, RelayCredentialEdit } from '@/types/setting'
import { canonicalRelayUrl, outcomeToStatus, type ProbeStatus } from './relay-probe'
export interface RelayEditorOptions {
  initialUrl: string
  initialCredentialConfigured: boolean
  onSave: (url: string, credential: RelayCredentialEdit) => Promise<CustomRelayMutationResult>
  onRemove: () => void | Promise<void>
}

const IDLE: ProbeStatus = { kind: 'idle' }

function isPresentedByParent(error: unknown): boolean {
  return (
    error instanceof Error &&
    'presented' in error &&
    (error as Error & { presented?: boolean }).presented === true
  )
}

export function useRelayEditor({
  initialUrl,
  initialCredentialConfigured,
  onSave,
  onRemove,
}: RelayEditorOptions) {
  const { t } = useTranslation()
  const [url, setUrl] = useState(initialUrl)
  const [accessToken, setAccessToken] = useState('')
  const [removeSavedToken, setRemoveSavedToken] = useState(false)
  const [visible, setVisible] = useState(false)
  const [probeStatus, setProbeStatus] = useState<ProbeStatus>(IDLE)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const probeGenerationRef = useRef(0)
  const trimmedUrl = url.trim()
  const canonicalUrl = canonicalRelayUrl(trimmedUrl)
  const canonicalInitialUrl = canonicalRelayUrl(initialUrl)
  const hasUrlChanged =
    canonicalUrl === null || canonicalInitialUrl === null || canonicalUrl !== canonicalInitialUrl
  const canTest = trimmedUrl.length > 0 && probeStatus.kind !== 'testing' && !saving
  const canSave = probeStatus.kind === 'success' && !saving

  const resetProbe = () => {
    probeGenerationRef.current += 1
    setProbeStatus(IDLE)
    setError(null)
  }

  const updateUrl = (nextUrl: string) => {
    setUrl(nextUrl)
    if (nextUrl.trim() !== initialUrl) setRemoveSavedToken(false)
    resetProbe()
  }

  const updateAccessToken = (nextToken: string) => {
    setAccessToken(nextToken)
    resetProbe()
  }

  const isCurrentProbe = (current: ProbeStatus, generation: number, pendingUrl: string) =>
    probeGenerationRef.current === generation &&
    current.kind === 'testing' &&
    current.pendingUrl === pendingUrl

  const testAvailability = async () => {
    if (!canTest) return
    const generation = probeGenerationRef.current + 1
    probeGenerationRef.current = generation
    setError(null)
    setProbeStatus({ kind: 'testing', pendingUrl: trimmedUrl })
    try {
      const credential = accessToken
        ? { mode: 'override' as const, accessToken }
        : removeSavedToken || hasUrlChanged || !initialCredentialConfigured
          ? { mode: 'none' as const }
          : { mode: 'stored' as const }
      const outcome = await probeRelayUrl(trimmedUrl, credential)
      setProbeStatus(current => {
        if (!isCurrentProbe(current, generation, trimmedUrl)) return current
        return outcomeToStatus(outcome, t)
      })
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err)
      setProbeStatus(current => {
        if (!isCurrentProbe(current, generation, trimmedUrl)) return current
        return {
          kind: 'failure',
          message: t('settings.sections.network.customRelays.testErrors.unavailable', {
            defaultValue: message,
          }),
        }
      })
    }
  }

  const saveRelay = async () => {
    if (!canSave) return
    setSaving(true)
    setError(null)
    try {
      const credential: RelayCredentialEdit = removeSavedToken
        ? { action: 'delete' }
        : accessToken
          ? { action: 'set', accessToken }
          : { action: 'keep' }
      await onSave(trimmedUrl, credential)
      setAccessToken('')
      setVisible(false)
      setRemoveSavedToken(false)
    } catch (err) {
      if (!isPresentedByParent(err)) {
        setError(
          t('settings.sections.network.customRelays.credentials.saveError', {
            message: err instanceof Error ? err.message : String(err),
          })
        )
      }
    } finally {
      setSaving(false)
    }
  }

  const removeRelay = async () => {
    if (saving) return
    setSaving(true)
    setError(null)
    try {
      await onRemove()
    } catch (err) {
      if (!isPresentedByParent(err)) {
        setError(
          t('settings.sections.network.customRelays.saveError', {
            message: err instanceof Error ? err.message : String(err),
          })
        )
      }
    } finally {
      setSaving(false)
    }
  }

  const toggleTokenRemoval = () => {
    setRemoveSavedToken(value => !value)
    setAccessToken('')
    resetProbe()
  }
  return {
    url,
    accessToken,
    configured: initialCredentialConfigured,
    removeSavedToken,
    visible,
    probeStatus,
    saving,
    error,
    hasUrlChanged,
    canTest,
    canSave,
    updateUrl,
    updateAccessToken,
    testAvailability,
    saveRelay,
    removeRelay,
    toggleTokenRemoval,
    toggleVisible: () => setVisible(value => !value),
  }
}
