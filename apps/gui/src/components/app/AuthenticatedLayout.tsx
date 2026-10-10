import { AnimatePresence, m } from 'framer-motion'
import { useLocation, useOutlet } from 'react-router'
import { useReducedMotion } from '@/hooks/useVisualEffects'
import { LibraryMainLayout } from '@/layouts'

const PAGE_TRANSITION = { duration: 0.16, ease: [0.22, 1, 0.36, 1] } as const

export function AuthenticatedLayout() {
  const routerLocation = useLocation()
  const outlet = useOutlet()
  const reduceMotion = useReducedMotion()
  const pageTransitionsDisabled = reduceMotion || import.meta.env.VITE_E2E === '1'

  return (
    <LibraryMainLayout>
      <div className="relative h-full overflow-hidden">
        <AnimatePresence initial={false} mode="sync">
          <m.div
            key={routerLocation.pathname}
            initial={pageTransitionsDisabled ? false : { opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={pageTransitionsDisabled ? undefined : { opacity: 0 }}
            transition={pageTransitionsDisabled ? { duration: 0 } : PAGE_TRANSITION}
            className="absolute inset-0"
          >
            {outlet}
          </m.div>
        </AnimatePresence>
      </div>
    </LibraryMainLayout>
  )
}
