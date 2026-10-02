import { useTranslation } from 'react-i18next'
import type { SettingsCategory } from '@/components/setting/settings-config'

export default function SettingsPageHeader({ category }: { category: SettingsCategory['id'] }) {
  const { t } = useTranslation()
  return (
    <header className="mb-6" data-testid="settings-page-header">
      <h1 className="text-ui-title font-semibold break-words">
        {t(`settings.categories.${category}`)}
      </h1>
      <p className="mt-1 text-ui-body-relaxed text-muted-foreground break-words">
        {t(`settings.pageDescriptions.${category}`)}
      </p>
    </header>
  )
}
