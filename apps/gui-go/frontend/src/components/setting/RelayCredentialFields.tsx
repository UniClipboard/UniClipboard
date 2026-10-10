import { Eye, EyeOff, KeyRound, TriangleAlert } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Button, Input } from '@/components/ui'
import { RelayCredentialStatus } from './RelayCredentialStatus'
import type { useRelayEditor } from './useRelayEditor'

type CredentialFieldsProps = Pick<
  ReturnType<typeof useRelayEditor>,
  | 'accessToken'
  | 'configured'
  | 'removeSavedToken'
  | 'visible'
  | 'saving'
  | 'hasUrlChanged'
  | 'updateAccessToken'
  | 'toggleTokenRemoval'
  | 'toggleVisible'
> & { displayIndex: number; initialUrl: string }
export function RelayCredentialFields({
  displayIndex,
  initialUrl,
  accessToken,
  configured,
  removeSavedToken,
  visible,
  saving,
  hasUrlChanged,
  updateAccessToken,
  toggleTokenRemoval,
  toggleVisible,
}: CredentialFieldsProps) {
  const { t } = useTranslation()
  return (
    <div className="mt-3 space-y-1.5">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <label htmlFor={`relay-access-token-${displayIndex}`} className="text-ui-body font-medium">
          {t('settings.sections.network.customRelays.tokenLabel')}
        </label>
        {!hasUrlChanged && initialUrl && <RelayCredentialStatus configured={configured} />}
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
          className="h-9 border-border/60 bg-muted/20 pr-10 font-mono shadow-none"
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
          onClick={toggleVisible}
        >
          {visible ? <EyeOff aria-hidden="true" /> : <Eye aria-hidden="true" />}
        </Button>
      </div>
      {!hasUrlChanged && configured && (
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="h-7 px-1.5 text-muted-foreground hover:text-destructive"
          disabled={saving}
          onClick={toggleTokenRemoval}
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
          className="flex items-center gap-1.5 text-ui-caption font-medium text-amber-700 dark:text-amber-400"
          role="status"
        >
          <TriangleAlert aria-hidden="true" className="size-3.5" />
          {t('settings.sections.network.customRelays.pendingTokenRemoval')}
        </p>
      )}
    </div>
  )
}
