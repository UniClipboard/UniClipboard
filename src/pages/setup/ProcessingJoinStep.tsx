import { Loader2, Monitor, Smartphone } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import StepLayout from '@/pages/setup/StepLayout'
import type { ProcessingJoinStepProps } from '@/pages/setup/types'

export default function ProcessingJoinStep({
  onCancel,
  loading,
  direction,
}: ProcessingJoinStepProps) {
  const { t } = useTranslation(undefined, { keyPrefix: 'setup.page' })
  const { t: tCommon } = useTranslation(undefined, { keyPrefix: 'setup.common' })

  return (
    <StepLayout
      variant="centered"
      title={t('processingJoinSpace.title')}
      subtitle={t('processingJoinSpace.subtitle')}
      direction={direction}
      footer={
        <Button
          variant="ghost"
          className="text-muted-foreground"
          onClick={onCancel}
          disabled={loading}
        >
          {tCommon('cancel')}
        </Button>
      }
    >
      <div className="flex flex-col items-center justify-center py-8 sm:py-10">
        <div className="mb-4 flex h-14 w-14 items-center justify-center rounded-full bg-primary/10 sm:mb-5 sm:h-16 sm:w-16">
          <Loader2 className="h-8 w-8 animate-spin text-primary" />
        </div>
      </div>
      <div className="flex items-center justify-center">
        <div className="flex items-center gap-2.5 rounded-lg border border-border/50 bg-muted/40 px-4 py-2.5 text-sm text-muted-foreground sm:px-5 sm:py-3">
          <div className="flex shrink-0 items-center gap-1">
            <Monitor className="h-4 w-4" />
            <span className="text-xs">/</span>
            <Smartphone className="h-4 w-4" />
          </div>
          <span>{t('processingJoinSpace.hint')}</span>
        </div>
      </div>
    </StepLayout>
  )
}
