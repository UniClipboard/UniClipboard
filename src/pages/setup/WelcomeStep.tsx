import { motion } from 'framer-motion'
import { Shield, Smartphone, ArrowRight } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { WelcomeStepProps } from '@/pages/setup/types'

export default function WelcomeStep({ onCreate, onJoin, loading }: WelcomeStepProps) {
  const { t } = useTranslation(undefined, { keyPrefix: 'setup.welcome' })

  return (
    <motion.div
      initial={{ opacity: 0, y: 16 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: -16 }}
      className="w-full"
    >
      <div className="mb-12 text-center lg:text-left">
        <h1 className="text-3xl font-semibold tracking-tight text-foreground lg:text-4xl">
          {t('title')}
        </h1>
        <p className="mt-4 text-lg text-muted-foreground">{t('subtitle')}</p>
      </div>

      <div className="grid grid-cols-2 gap-4">
        <button
          type="button"
          onClick={onCreate}
          disabled={loading}
          className="group relative flex flex-col items-start gap-6 rounded-xl border bg-card p-8 text-left shadow-sm transition-all duration-200 hover:-translate-y-1 hover:border-primary/50 hover:shadow-lg active:translate-y-0 active:shadow-sm disabled:opacity-50"
        >
          <div className="flex h-12 w-12 items-center justify-center text-primary">
            <Shield className="h-7 w-7" />
          </div>
          <div className="space-y-2">
            <h3 className="text-lg font-medium text-foreground">{t('create.title')}</h3>
            <p className="text-sm leading-relaxed text-muted-foreground">
              {t('create.description')}
            </p>
          </div>
          <div className="mt-2 flex items-center gap-2 text-sm font-medium text-primary">
            {t('create.cta')}
            <ArrowRight className="h-4 w-4 transition-transform group-hover:translate-x-1" />
          </div>
        </button>

        <button
          type="button"
          onClick={onJoin}
          disabled={loading}
          className="group relative flex flex-col items-start gap-6 rounded-xl border bg-card p-8 text-left shadow-sm transition-all duration-200 hover:-translate-y-1 hover:border-primary/50 hover:shadow-lg active:translate-y-0 active:shadow-sm disabled:opacity-50"
        >
          <div className="flex h-12 w-12 items-center justify-center text-primary">
            <Smartphone className="h-7 w-7" />
          </div>
          <div className="space-y-2">
            <h3 className="text-lg font-medium text-foreground">{t('join.title')}</h3>
            <p className="text-sm leading-relaxed text-muted-foreground">{t('join.description')}</p>
          </div>
          <div className="mt-2 flex items-center gap-2 text-sm font-medium text-primary">
            {t('join.cta')}
            <ArrowRight className="h-4 w-4 transition-transform group-hover:translate-x-1" />
          </div>
        </button>
      </div>

      <div className="mt-12 text-center text-xs text-muted-foreground lg:text-left">
        {t('footer')}
      </div>
    </motion.div>
  )
}
