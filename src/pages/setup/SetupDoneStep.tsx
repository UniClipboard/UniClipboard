import { CheckCircle2, ArrowRight } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import StepLayout from '@/pages/setup/StepLayout'
import { SetupDoneStepProps } from '@/pages/setup/types'

export default function SetupDoneStep({ onComplete, loading, direction }: SetupDoneStepProps) {
  const { t } = useTranslation(undefined, { keyPrefix: 'setup.done' })

  const enterButton = (
    <Button onClick={onComplete} disabled={loading} className="min-w-40">
      {t('actions.enter')}
      <ArrowRight className="ml-2 h-4 w-4" />
    </Button>
  )

  return (
    <StepLayout
      variant="centered"
      title={t('title')}
      subtitle={t('subtitle')}
      footer={enterButton}
      direction={direction}
    >
      <div className="flex justify-center">
        <div className="mb-6 flex h-16 w-16 items-center justify-center text-green-500 sm:mb-8 sm:h-20 sm:w-20">
          <CheckCircle2 className="h-12 w-12 sm:h-16 sm:w-16" />
        </div>
      </div>
    </StepLayout>
  )
}
