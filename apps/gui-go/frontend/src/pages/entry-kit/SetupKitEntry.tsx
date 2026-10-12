import { useTranslation } from 'react-i18next'
import { ChoiceList, EntryPage, Text } from '@/ui-kit'
import appIcon from '@/updater/app-icon.png'
import { EntryKitFrame } from './EntryKitFrame'

/** S1 · where this device fits: create a space, join one, or move a device backup. */
export function SetupKitEntry({
  onCreate,
  onJoin,
  onImport,
  loading,
}: {
  onCreate: () => void
  onJoin: () => void
  onImport: () => void
  loading?: boolean
}) {
  const { t } = useTranslation(undefined, { keyPrefix: 'setup.welcome' })
  return (
    <EntryKitFrame>
      <EntryPage appIcon={appIcon} title={t('title')} description={t('subtitle')}>
        <ChoiceList
          label={t('title')}
          disabled={loading}
          items={[
            {
              id: 'create',
              'data-testid': 'setup-entry-create',
              title: t('create.title'),
              description: t('create.description'),
              icon: 'shield',
            },
            {
              id: 'join',
              'data-testid': 'setup-entry-join',
              title: t('join.title'),
              description: t('join.description'),
              icon: 'phone',
            },
          ]}
          onSelect={id => (id === 'create' ? onCreate() : onJoin())}
        />
        <ChoiceList
          label={t('import.divider')}
          title={t('import.divider')}
          disabled={loading}
          items={[
            {
              id: 'import',
              'data-testid': 'setup-entry-import',
              title: t('import.title'),
              description: t('import.description'),
              icon: 'package',
            },
          ]}
          onSelect={onImport}
        />
        <Text role="caption" tone="secondary">
          {t('footer')}
        </Text>
      </EntryPage>
    </EntryKitFrame>
  )
}
