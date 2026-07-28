import {
  Check,
  Eye,
  EyeOff,
  KeyRound,
  Loader2,
  Save,
  SignalHigh,
  Trash2,
  TriangleAlert,
} from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  getRelayCredentialStatus,
  probeRelayUrl,
  type RelayProbeOutcome,
} from '@/api/daemon/settings'
import { Button, Input } from '@/components/ui'
import { cn } from '@/lib/utils'
import type { RelayCredentialEdit, RelaySaveContextResult } from '@/types/setting'

interface RelayEditorProps {
  index: number
  initialUrl: string
  removable: boolean
  onSave: (url: string, credential: RelayCredentialEdit) => Promise<RelaySaveContextResult>
  onRemove: () => void | Promise<void>
}

type ProbeStatus =
  | { kind: 'idle' }
  | { kind: 'testing'; pendingUrl: string }
  | { kind: 'success'; latencyMs: number }
  | { kind: 'failure'; message: string }

const IDLE: ProbeStatus = { kind: 'idle' }

function canonicalRelayUrl(value: string): string | null {
  try {
    return new URL(value.trim()).toString()
  } catch {
    return null
  }
}

export function RelayEditor({ index, initialUrl, removable, onSave, onRemove }: RelayEditorProps) {
  const { t } = useTranslation()
  const [url, setUrl] = useState(initialUrl)
  const [accessToken, setAccessToken] = useState('')
  const [configured, setConfigured] = useState<boolean | null>(initialUrl ? null : false)
  const [removeSavedToken, setRemoveSavedToken] = useState(false)
  const [visible, setVisible] = useState(false)
  const [probeStatus, setProbeStatus] = useState<ProbeStatus>(IDLE)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const probeGenerationRef = useRef(0)
  const displayIndex = index + 1
  const trimmedUrl = url.trim()
  const canonicalUrl = canonicalRelayUrl(trimmedUrl)
  const canonicalInitialUrl = canonicalRelayUrl(initialUrl)
  const hasUrlChanged =
    canonicalUrl === null || canonicalInitialUrl === null || canonicalUrl !== canonicalInitialUrl
  const waitingForCredentialStatus =
    Boolean(initialUrl) &&
    !hasUrlChanged &&
    !accessToken &&
    !removeSavedToken &&
    configured === null
  const canTest =
    trimmedUrl.length > 0 &&
    probeStatus.kind !== 'testing' &&
    !saving &&
    !waitingForCredentialStatus
  const canSave = probeStatus.kind === 'success' && !saving

  useEffect(() => {
    if (!initialUrl) return
    let current = true
    void getRelayCredentialStatus(initialUrl).then(
      status => {
        if (current) setConfigured(status.configured)
      },
      err => {
        if (!current) return
        setConfigured(false)
        setError(
          t('settings.sections.network.customRelays.credentials.loadError', {
            message: err instanceof Error ? err.message : String(err),
          })
        )
      }
    )
    return () => {
      current = false
    }
  }, [initialUrl, t])

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

  const testAvailability = async () => {
    if (!canTest) return
    const generation = probeGenerationRef.current + 1
    probeGenerationRef.current = generation
    setError(null)
    setProbeStatus({ kind: 'testing', pendingUrl: trimmedUrl })
    try {
      const credential = accessToken
        ? { mode: 'override' as const, accessToken }
        : removeSavedToken || hasUrlChanged || configured !== true
          ? { mode: 'none' as const }
          : { mode: 'stored' as const }
      const outcome = await probeRelayUrl(trimmedUrl, credential)
      setProbeStatus(current => {
        if (
          probeGenerationRef.current !== generation ||
          current.kind !== 'testing' ||
          current.pendingUrl !== trimmedUrl
        )
          return current
        return outcomeToStatus(outcome, t)
      })
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err)
      setProbeStatus(current => {
        if (
          probeGenerationRef.current !== generation ||
          current.kind !== 'testing' ||
          current.pendingUrl !== trimmedUrl
        )
          return current
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
      const result = await onSave(trimmedUrl, credential)
      setConfigured(result.credentialStatus.configured)
      setAccessToken('')
      setVisible(false)
      setRemoveSavedToken(false)
    } catch (err) {
      setError(
        t('settings.sections.network.customRelays.credentials.saveError', {
          message: err instanceof Error ? err.message : String(err),
        })
      )
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
      setError(
        t('settings.sections.network.customRelays.saveError', {
          message: err instanceof Error ? err.message : String(err),
        })
      )
    } finally {
      setSaving(false)
    }
  }

  return (
    <section className="py-4" aria-labelledby={`relay-node-${displayIndex}-title`}>
      <div className="flex items-center justify-between gap-3">
        <h5 id={`relay-node-${displayIndex}-title`} className="text-sm font-medium">
          {t('settings.sections.network.customRelays.rowLabel', { index: displayIndex })}
        </h5>
        <Button
          type="button"
          variant="ghost"
          size="icon-sm"
          className="text-muted-foreground hover:text-destructive"
          aria-label={t('settings.sections.network.customRelays.removeAriaLabel', {
            index: displayIndex,
          })}
          disabled={!removable || saving}
          onClick={() => void removeRelay()}
        >
          <Trash2 aria-hidden="true" />
        </Button>
      </div>

      <div className="mt-3 space-y-1.5">
        <label htmlFor={`custom-relay-url-${displayIndex}`} className="text-xs font-medium">
          {t('settings.sections.network.customRelays.urlLabel')}
        </label>
        <Input
          id={`custom-relay-url-${displayIndex}`}
          type="url"
          inputMode="url"
          autoComplete="off"
          value={url}
          placeholder={t('settings.sections.network.customRelays.placeholder')}
          aria-label={t('settings.sections.network.customRelays.itemAriaLabel', {
            index: displayIndex,
          })}
          className="h-9 border-border/60 bg-background/50 font-mono text-xs shadow-none"
          disabled={saving}
          onChange={event => updateUrl(event.target.value)}
        />
      </div>

      <div className="mt-3 space-y-1.5">
        <div className="flex items-center justify-between gap-3">
          <label htmlFor={`relay-access-token-${displayIndex}`} className="text-xs font-medium">
            {t('settings.sections.network.customRelays.tokenLabel')}
          </label>
          {!hasUrlChanged && initialUrl && (
            <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
              <span
                aria-hidden="true"
                className={cn(
                  'size-1.5 rounded-full',
                  configured === null
                    ? 'animate-pulse bg-muted-foreground/40'
                    : configured
                      ? 'bg-emerald-500'
                      : 'bg-muted-foreground/40'
                )}
              />
              {configured === null
                ? t('settings.sections.network.customRelays.credentials.loading')
                : configured
                  ? t('settings.sections.network.customRelays.credentials.configured')
                  : t('settings.sections.network.customRelays.credentials.notConfigured')}
            </span>
          )}
        </div>
        <div className="relative">
          <Input
            id={`relay-access-token-${displayIndex}`}
            type={visible ? 'text' : 'password'}
            autoComplete="new-password"
            value={accessToken}
            aria-label={t('settings.sections.network.customRelays.credentials.inputAriaLabel', {
              index: displayIndex,
            })}
            placeholder={t('settings.sections.network.customRelays.credentials.placeholder')}
            className="h-9 border-border/60 bg-background/50 pr-10 font-mono text-xs shadow-none"
            disabled={saving || removeSavedToken}
            onChange={event => updateAccessToken(event.target.value)}
          />
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            className="absolute right-1 top-1/2 -translate-y-1/2"
            aria-label={t('settings.sections.network.customRelays.credentials.toggleVisibility')}
            disabled={saving || removeSavedToken || !accessToken}
            onClick={() => setVisible(value => !value)}
          >
            {visible ? <EyeOff aria-hidden="true" /> : <Eye aria-hidden="true" />}
          </Button>
        </div>
        {!hasUrlChanged && configured && (
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="h-7 px-1.5 text-xs text-muted-foreground hover:text-destructive"
            disabled={saving}
            onClick={() => {
              setRemoveSavedToken(value => !value)
              setAccessToken('')
              resetProbe()
            }}
          >
            <KeyRound aria-hidden="true" />
            {t(
              removeSavedToken
                ? 'settings.sections.network.customRelays.cancelRemoveSavedToken'
                : 'settings.sections.network.customRelays.removeSavedToken'
            )}
          </Button>
        )}
        {removeSavedToken && (
          <p
            className="flex items-center gap-1.5 text-xs font-medium text-amber-700 dark:text-amber-400"
            role="status"
          >
            <TriangleAlert aria-hidden="true" className="size-3.5" />
            {t('settings.sections.network.customRelays.pendingTokenRemoval')}
          </p>
        )}
      </div>

      <div className="mt-4 border-t border-border/40 pt-3">
        <p className="text-xs text-muted-foreground">
          {t('settings.sections.network.customRelays.testHint')}
        </p>
        <Button
          type="button"
          variant="outline"
          size="sm"
          className="mt-2"
          disabled={!canTest}
          onClick={() => void testAvailability()}
        >
          {probeStatus.kind === 'testing' ? (
            <Loader2 aria-hidden="true" className="animate-spin" />
          ) : (
            <SignalHigh aria-hidden="true" />
          )}
          {t('settings.sections.network.customRelays.testButton')}
        </Button>
        <ProbeResult status={probeStatus} />
      </div>

      <div className="mt-4 flex justify-end border-t border-border/40 pt-3">
        <Button type="button" size="sm" disabled={!canSave} onClick={() => void saveRelay()}>
          {saving ? (
            <Loader2 aria-hidden="true" className="animate-spin" />
          ) : (
            <Save aria-hidden="true" />
          )}
          {t('settings.sections.network.customRelays.saveButton')}
        </Button>
      </div>

      {error && (
        <p className="mt-2 text-xs text-destructive" role="alert">
          {error}
        </p>
      )}
    </section>
  )
}

interface ProbeResultProps {
  status: ProbeStatus
}

function ProbeResult({ status }: ProbeResultProps) {
  const { t } = useTranslation()
  if (status.kind === 'idle' || status.kind === 'testing') return null
  const success = status.kind === 'success'
  const message = success
    ? t('settings.sections.network.customRelays.testSuccess', { latencyMs: status.latencyMs })
    : status.message
  return (
    <p
      className={cn(
        'mt-2 flex items-center gap-1.5 text-xs',
        success ? 'text-emerald-700 dark:text-emerald-400' : 'text-destructive'
      )}
      role={success ? 'status' : 'alert'}
    >
      {success ? (
        <Check aria-hidden="true" className="size-3.5" />
      ) : (
        <TriangleAlert aria-hidden="true" className="size-3.5" />
      )}
      {message}
    </p>
  )
}

function outcomeToStatus(
  outcome: RelayProbeOutcome,
  t: (key: string, options?: Record<string, unknown>) => string
): ProbeStatus {
  switch (outcome.kind) {
    case 'success':
      return { kind: 'success', latencyMs: outcome.latencyMs }
    case 'invalidUrl':
      return {
        kind: 'failure',
        message: t('settings.sections.network.customRelays.testErrors.invalidUrl', {
          message: outcome.message,
        }),
      }
    case 'dns':
      return {
        kind: 'failure',
        message: t('settings.sections.network.customRelays.testErrors.dns', {
          message: outcome.message,
        }),
      }
    case 'tls':
      return {
        kind: 'failure',
        message: t('settings.sections.network.customRelays.testErrors.tls', {
          message: outcome.message,
        }),
      }
    case 'handshake':
      return {
        kind: 'failure',
        message: t('settings.sections.network.customRelays.testErrors.handshake', {
          message: outcome.message,
        }),
      }
    case 'timeout':
      return {
        kind: 'failure',
        message: t('settings.sections.network.customRelays.testErrors.timeout'),
      }
    case 'other':
      return {
        kind: 'failure',
        message: t('settings.sections.network.customRelays.testErrors.other', {
          message: outcome.message,
        }),
      }
  }
}
