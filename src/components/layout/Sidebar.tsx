import { motion } from 'framer-motion'
import { ArrowUpCircle, Home, MessageSquare, Monitor, Settings } from 'lucide-react'
import React, { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link, useLocation, useNavigate } from 'react-router-dom'
import { FeedbackDialog } from '@/components/feedback/FeedbackDialog'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { Progress } from '@/components/ui/progress'
import { toast } from '@/components/ui/toast'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip'
import { ReleaseNotes } from '@/components/update/ReleaseNotes'
import { useSetting } from '@/hooks/useSetting'
import { useUpdate } from '@/hooks/useUpdate'
import { createLogger } from '@/lib/logger'
import { cn } from '@/lib/utils'
import { sentryEnabled } from '@/observability/sentry'

const log = createLogger('sidebar')

const NavButton: React.FC<{
  to: string
  icon: React.ComponentType<{ className?: string }>
  label: string
  isActive: boolean
  layoutId: string
  onClick?: (e: React.MouseEvent<HTMLAnchorElement>) => void
  'data-settings-icon'?: boolean
}> = ({
  to,
  icon: Icon,
  label,
  isActive,
  layoutId,
  onClick,
  'data-settings-icon': dataSettingsIcon,
}) => {
  return (
    <TooltipProvider delayDuration={0}>
      <Tooltip>
        <TooltipTrigger asChild>
          <Link
            data-tauri-drag-region="false"
            data-settings-icon={dataSettingsIcon || undefined}
            to={to}
            className="relative group"
            onClick={
              onClick
                ? e => {
                    e.preventDefault()
                    onClick(e)
                  }
                : undefined
            }
          >
            {isActive && (
              <motion.div
                layoutId={layoutId}
                className="absolute inset-0 bg-primary/10 dark:bg-primary/20 rounded-lg"
                initial={false}
                transition={{
                  type: 'spring',
                  stiffness: 500,
                  damping: 30,
                }}
              />
            )}
            <div
              className={cn(
                'relative flex items-center justify-center w-10 h-10 rounded-lg transition-colors duration-200 z-10',
                isActive
                  ? 'text-primary'
                  : 'text-muted-foreground group-hover:text-primary group-hover:bg-muted'
              )}
            >
              <Icon className="w-5 h-5" />
            </div>
          </Link>
        </TooltipTrigger>
        <TooltipContent side="right" align="center" className="font-medium">
          <p>{label}</p>
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  )
}

interface SidebarProps {
  className?: string
}

const Sidebar: React.FC<SidebarProps> = ({ className }) => {
  const { t } = useTranslation()
  const location = useLocation()
  const navigate = useNavigate()
  const { setting } = useSetting()
  const [updateDialogOpen, setUpdateDialogOpen] = useState(false)
  const [feedbackOpen, setFeedbackOpen] = useState(false)
  const { updateInfo, isCheckingUpdate, installUpdate, downloadProgress } = useUpdate()
  const isInstallingUpdate = downloadProgress.phase !== 'idle'

  const navItems = [
    { to: '/', icon: Home, label: t('nav.dashboard') },
    { to: '/devices', icon: Monitor, label: t('nav.devices') },
  ]

  useEffect(() => {
    if (!setting?.general.autoCheckUpdate) {
      setUpdateDialogOpen(false)
    }
  }, [setting?.general.autoCheckUpdate])

  const handleInstallUpdate = async () => {
    if (!updateInfo || isInstallingUpdate) return
    try {
      await installUpdate()
      setUpdateDialogOpen(false)
    } catch (error) {
      log.error({ err: error }, '更新失败')
      toast.error(t('update.installFailed'))
    }
  }

  return (
    <>
      <aside
        data-tauri-drag-region
        className={cn(
          'relative z-10 w-14 h-full shrink-0 flex flex-col items-center py-4',
          'bg-transparent',
          className
        )}
      >
        {/* Main Navigation */}
        <div className="relative z-10 flex flex-col gap-3 w-full items-center">
          {navItems.map(item => (
            <NavButton
              key={item.to}
              to={item.to}
              icon={item.icon}
              label={item.label}
              isActive={location.pathname === item.to}
              layoutId="sidebar-nav-top"
            />
          ))}
        </div>

        <div data-tauri-drag-region className="flex-1 w-full min-h-0" />

        {/* Bottom Navigation */}
        <div className="relative z-10 flex flex-col gap-3 w-full items-center">
          {updateInfo && (
            <TooltipProvider delayDuration={0}>
              <Tooltip>
                <TooltipTrigger asChild>
                  <button
                    type="button"
                    aria-label={t('nav.updateAvailable')}
                    data-tauri-drag-region="false"
                    className="relative group"
                    onClick={() => setUpdateDialogOpen(true)}
                    disabled={isCheckingUpdate}
                  >
                    <div
                      className={cn(
                        'relative flex items-center justify-center w-10 h-10 rounded-lg transition-colors duration-200 z-10',
                        'text-amber-600 dark:text-amber-400 group-hover:bg-muted'
                      )}
                    >
                      <ArrowUpCircle className="w-5 h-5" />
                      <span className="absolute top-2.5 right-2.5 flex h-2 w-2">
                        <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-amber-500/70 opacity-75" />
                        <span className="relative inline-flex h-2 w-2 rounded-full bg-amber-500" />
                      </span>
                    </div>
                  </button>
                </TooltipTrigger>
                <TooltipContent side="right" align="center" className="font-medium">
                  <p>{t('nav.updateAvailable')}</p>
                </TooltipContent>
              </Tooltip>
            </TooltipProvider>
          )}
          {sentryEnabled && (
            <>
              <TooltipProvider delayDuration={0}>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <button
                      type="button"
                      aria-label={t('nav.feedback')}
                      data-tauri-drag-region="false"
                      className="relative group"
                      onClick={() => setFeedbackOpen(true)}
                    >
                      <div
                        className={cn(
                          'relative flex items-center justify-center w-10 h-10 rounded-lg transition-colors duration-200 z-10',
                          'text-muted-foreground group-hover:text-primary group-hover:bg-muted'
                        )}
                      >
                        <MessageSquare className="w-5 h-5" />
                      </div>
                    </button>
                  </TooltipTrigger>
                  <TooltipContent side="right" align="center" className="font-medium">
                    <p>{t('nav.feedback')}</p>
                  </TooltipContent>
                </Tooltip>
              </TooltipProvider>
              <FeedbackDialog open={feedbackOpen} onOpenChange={setFeedbackOpen} />
            </>
          )}
          <NavButton
            to="/settings"
            icon={Settings}
            label={t('nav.settings')}
            isActive={location.pathname.startsWith('/settings')}
            layoutId="sidebar-nav-bottom"
            onClick={() => {
              if (location.pathname.startsWith('/settings')) return
              navigate('/settings')
            }}
          />
        </div>
      </aside>
      <AlertDialog open={updateDialogOpen} onOpenChange={setUpdateDialogOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('update.title')}</AlertDialogTitle>
            <AlertDialogDescription asChild>
              <div className="space-y-3">
                <div className="space-y-1 text-sm">
                  <div className="flex items-center justify-between text-muted-foreground">
                    <span>{t('update.currentVersion')}</span>
                    <span className="text-foreground">{updateInfo?.currentVersion ?? '-'}</span>
                  </div>
                  <div className="flex items-center justify-between text-muted-foreground">
                    <span>{t('update.latestVersion')}</span>
                    <span className="text-foreground">{updateInfo?.version ?? '-'}</span>
                  </div>
                </div>
                <div className="space-y-2">
                  <div className="text-sm font-medium text-foreground">
                    {t('update.releaseNotes')}
                  </div>
                  <div className="max-h-48 overflow-auto rounded-md border border-border/60 bg-muted/30 px-3 py-2 text-sm text-muted-foreground">
                    <ReleaseNotes content={updateInfo?.body ?? ''} fallback={t('update.noNotes')} />
                  </div>
                </div>
                {downloadProgress.phase !== 'idle' && (
                  <div className="space-y-2 pt-2">
                    <div className="flex justify-between text-xs text-muted-foreground">
                      <span>
                        {downloadProgress.phase === 'installing'
                          ? t('update.installing')
                          : t('update.downloading')}
                      </span>
                      {downloadProgress.total !== null && (
                        <span>
                          {Math.round((downloadProgress.downloaded / downloadProgress.total) * 100)}
                          %
                        </span>
                      )}
                    </div>
                    <Progress
                      value={
                        downloadProgress.total !== null
                          ? (downloadProgress.downloaded / downloadProgress.total) * 100
                          : undefined
                      }
                      className={cn('h-2', downloadProgress.total === null && 'animate-pulse')}
                    />
                  </div>
                )}
              </div>
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={isInstallingUpdate}>{t('update.later')}</AlertDialogCancel>
            <AlertDialogAction
              onClick={event => {
                event.preventDefault()
                handleInstallUpdate()
              }}
              disabled={isInstallingUpdate}
            >
              {t('update.updateNow')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  )
}

export default Sidebar
