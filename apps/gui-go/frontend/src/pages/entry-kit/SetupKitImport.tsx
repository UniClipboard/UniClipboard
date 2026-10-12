import { useId, useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { useConfigImport, type ConfigImportErrorKind } from '@/hooks/useConfigImport'
import { Button, ButtonRow, EntryPage, FormField, InlineNotice, ProgressBar, Text } from '@/ui-kit'
import appIcon from '@/updater/app-icon.png'
import { EntryKitFacts } from './EntryKitFacts'
import { EntryKitFrame } from './EntryKitFrame'

const FORM = 'setup-import-form'

/** Trailing path segment of an absolute file path (POSIX or Windows). */
function baseName(path: string): string {
  const parts = path.split(/[/\\]/)
  return parts[parts.length - 1] || path
}

/**
 * S10–S12 · move a device backup: choose the `.ucbundle`, unlock it with the exported device's
 * space passphrase, confirm the identity move, then stage and restart (`useConfigImport`, the same
 * owner as the existing setup screen and the settings import).
 */
export function SetupKitImport({ onBack }: { onBack: () => void }) {
  const { t } = useTranslation(undefined, { keyPrefix: 'setup.importConfig' })
  const { t: tWelcome } = useTranslation(undefined, { keyPrefix: 'setup.welcome' })
  const { t: tUnlock } = useTranslation(undefined, { keyPrefix: 'unlock.passphraseModal' })
  const [errorKind, setErrorKind] = useState<ConfigImportErrorKind | null>(null)
  const imp = useConfigImport({ onError: setErrorKind })
  const fileLabelId = useId()
  const errorMessage = errorKind ? t(`errors.${errorKind}`) : null
  const fileName = imp.sourcePath ? baseName(imp.sourcePath) : null
  const status = { text: tWelcome('import.title'), icon: 'package' as const }

  if (imp.isRestarting) {
    return (
      <EntryKitFrame>
        <EntryPage
          appIcon={appIcon}
          status={{ ...status, busy: true }}
          title={t('restartingTitle')}
          description={t('restartingDescription')}
        >
          <ProgressBar label={t('restartingTitle')} />
          {imp.stagedResult?.unlockRequiredAfterApply && (
            <Text role="caption" tone="secondary">
              {t('restartingUnlockHint')}
            </Text>
          )}
        </EntryPage>
      </EntryKitFrame>
    )
  }

  if (imp.phase === 'confirm') {
    const sourceMode = imp.preview?.sourceMode
    return (
      <EntryKitFrame>
        <EntryPage
          appIcon={appIcon}
          status={status}
          title={t('title')}
          description={t('confirmSubtitle')}
          back={{ label: t('actions.back'), onClick: imp.back, disabled: imp.busy }}
          actions={
            <Button
              variant="primary"
              busy={imp.busy}
              onClick={() => {
                setErrorKind(null)
                void imp.confirmImport()
              }}
            >
              {t(imp.busy ? 'actions.staging' : 'actions.import')}
            </Button>
          }
        >
          {errorMessage && <InlineNotice tone="error" title={errorMessage} />}
          <InlineNotice tone="warning" title={t('note')} />
          {imp.preview && (
            <EntryKitFacts
              label={t('metaTitle')}
              facts={[
                { id: 'version', label: t('metaAppVersion'), value: imp.preview.appVersion },
                {
                  id: 'source',
                  label: t('metaSourceMode'),
                  value:
                    sourceMode === 'portable'
                      ? t('metaSourcePortable')
                      : sourceMode === 'installed'
                        ? t('metaSourceInstalled')
                        : (sourceMode ?? ''),
                },
                {
                  id: 'fingerprint',
                  label: t('metaFingerprint'),
                  value: imp.preview.deviceFingerprint,
                  monospace: true,
                },
              ]}
            />
          )}
        </EntryPage>
      </EntryKitFrame>
    )
  }

  const canContinue = !!imp.sourcePath && !!imp.password && !imp.busy
  const submit = (event: FormEvent) => {
    event.preventDefault()
    if (!canContinue) return
    setErrorKind(null)
    void imp.submitPassword()
  }

  return (
    <EntryKitFrame>
      <EntryPage
        appIcon={appIcon}
        status={status}
        title={t('title')}
        description={t('subtitle')}
        back={{ label: t('actions.back'), onClick: onBack, disabled: imp.busy }}
        actions={
          <Button variant="primary" submitForm={FORM} busy={imp.busy} disabled={!canContinue}>
            {t(imp.busy ? 'actions.staging' : 'actions.continue')}
          </Button>
        }
      >
        <form id={FORM} onSubmit={submit}>
          {errorMessage && <InlineNotice tone="error" title={errorMessage} />}
          <div role="group" aria-labelledby={fileLabelId} className="flex flex-col gap-2">
            <span id={fileLabelId}>
              <Text emphasized>{t('fileLabel')}</Text>
            </span>
            <ButtonRow>
              <span data-testid="setup-import-pick">
                <Button
                  icon="file"
                  disabled={imp.busy}
                  onClick={() => {
                    setErrorKind(null)
                    void imp.pickFile()
                  }}
                >
                  {fileName ?? t('chooseFile')}
                </Button>
              </span>
            </ButtonRow>
          </div>
          {imp.sourcePath && (
            <FormField
              label={t('passwordLabel')}
              type="password"
              reveal={{ show: tUnlock('show'), hide: tUnlock('hide') }}
              autoComplete="off"
              autoFocus
              value={imp.password}
              onValueChange={value => {
                setErrorKind(null)
                imp.setPassword(value)
              }}
              disabled={imp.busy}
              placeholder={t('passwordPlaceholder')}
              hint={t('passwordHint')}
            />
          )}
        </form>
      </EntryPage>
    </EntryKitFrame>
  )
}
