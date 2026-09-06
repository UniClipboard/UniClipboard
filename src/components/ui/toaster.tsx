import { useEffect, useSyncExternalStore } from 'react'
import { useTranslation } from 'react-i18next'
import { AnimatedToastStack } from '@/components/motion/animated-toast-stack'
import { toast, toastStore } from './toast'

export function Toaster() {
  const toasts = useSyncExternalStore(toastStore.subscribe, toastStore.getSnapshot)
  const { t } = useTranslation()

  useEffect(() => {
    const timers = toasts.flatMap(item => {
      const duration = item.duration ?? 0
      if (duration <= 0 || !Number.isFinite(duration)) return []
      const remaining = Math.max(0, duration - (Date.now() - (item.createdAt ?? Date.now())))
      return [window.setTimeout(() => toast.dismiss(item.id), remaining)]
    })
    return () => timers.forEach(timer => window.clearTimeout(timer))
  }, [toasts])

  return (
    <AnimatedToastStack
      toasts={toasts}
      onDismiss={toast.dismiss}
      placement="fixed"
      position="bottom-right"
      label={t('common.notifications', 'Notifications')}
      dismissLabel={t('common.dismissNotification', 'Dismiss toast')}
    />
  )
}
