import { ShieldAlert } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import type { UpdateMetadata } from '@/api/updater'
import { ReleaseNotes } from '@/components/update/ReleaseNotes'

interface UpdateConfirmationNoticeProps {
  update: UpdateMetadata
  onConfirm: () => void
  confirming?: boolean
}

export function UpdateConfirmationNotice({
  update,
  onConfirm,
  confirming = false,
}: UpdateConfirmationNoticeProps) {
  const { t } = useTranslation()
  const confirmation = update.confirmation

  if (confirmation.status === 'not_required' || confirmation.status === 'confirmed') return null

  return (
    <div className="space-y-3 rounded-md border border-amber-500/30 bg-amber-500/10 p-3 text-left">
      <div className="flex items-center gap-2 font-medium text-amber-800 dark:text-amber-200">
        <ShieldAlert className="size-4 shrink-0" />
        <span>{t('update.confirmation.title')}</span>
      </div>
      {confirmation.status === 'pending' ? (
        <>
          <div className="max-h-44 overflow-auto text-sm text-foreground">
            <ReleaseNotes
              content={confirmation.description}
              fallback={t('update.confirmation.unavailable')}
            />
          </div>
          <button
            type="button"
            className="rounded-md border border-amber-600/40 bg-background px-3 py-1.5 text-sm font-medium text-foreground hover:bg-muted disabled:opacity-50"
            onClick={onConfirm}
            disabled={confirming}
          >
            {confirming
              ? t('update.confirmation.confirming')
              : t('update.confirmation.acknowledge')}
          </button>
        </>
      ) : (
        <p className="text-sm font-medium text-amber-900 dark:text-amber-100">
          {t('update.confirmation.unavailable')}
        </p>
      )}
    </div>
  )
}
