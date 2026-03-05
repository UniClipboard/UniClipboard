import { motion } from 'framer-motion'
import { AlertCircle } from 'lucide-react'
import type { StepLayoutProps } from '@/pages/setup/types'

const slideVariants = {
  enter: (direction: 'forward' | 'backward') => ({
    x: direction === 'forward' ? 20 : -20,
    opacity: 0,
  }),
  center: {
    x: 0,
    opacity: 1,
  },
  exit: (direction: 'forward' | 'backward') => ({
    x: direction === 'forward' ? -20 : 20,
    opacity: 0,
  }),
}

export default function StepLayout({
  headerLeft,
  headerRight,
  title,
  subtitle,
  children,
  footer,
  hint,
  error,
  variant = 'default',
  direction = 'forward',
}: StepLayoutProps) {
  const centered = variant === 'centered'

  return (
    <motion.div
      custom={direction}
      variants={slideVariants}
      initial="enter"
      animate="center"
      exit="exit"
      transition={{ duration: 0.2, ease: 'easeOut' }}
    >
      {(headerLeft || headerRight) && (
        <div data-testid="step-header" className="flex items-center justify-between">
          {headerLeft ?? <div />}
          {headerRight ?? <div />}
        </div>
      )}

      <div data-testid="step-title-section" className={centered ? 'text-center' : undefined}>
        <h1 className="text-2xl font-semibold tracking-tight text-foreground">{title}</h1>
        {subtitle && <p className="mt-2 text-muted-foreground">{subtitle}</p>}
      </div>

      {children}

      {error && (
        <motion.div
          role="alert"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          className={`mt-4 flex items-center gap-2 text-sm text-destructive sm:mt-5 ${centered ? 'justify-center' : ''}`}
        >
          <AlertCircle className="h-4 w-4 shrink-0" />
          <span>{error}</span>
        </motion.div>
      )}

      {footer && (
        <div
          data-testid="step-footer"
          className={`mt-7 flex sm:mt-8 ${centered ? 'justify-center' : ''}`}
        >
          {footer}
        </div>
      )}

      {hint && <p className="mt-4 text-xs text-muted-foreground sm:mt-5">{hint}</p>}
    </motion.div>
  )
}
