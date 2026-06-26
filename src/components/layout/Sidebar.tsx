import { m } from 'framer-motion'
import {
  ArrowUpCircle,
  Bug,
  Check,
  Layers,
  MessageSquare,
  Monitor,
  Settings,
  X,
} from 'lucide-react'
import React, { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link, useLocation, useNavigate } from 'react-router-dom'
import { updateDebugMode } from '@/api/daemon/diagnostics'
import {
  captureUpdateActionInvoked,
  captureUpdateDialogOpened,
  captureUpdateDismissed,
  type DismissSource,
  toUiPhase,
} from '@/api/update-telemetry'
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
import { PackageManagerUpdateDialog } from '@/components/update/PackageManagerUpdateDialog'
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
              <m.div
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
                'relative flex items-center justify-center size-10 rounded-lg transition-colors duration-200 z-10',
                isActive
                  ? 'text-primary'
                  : 'text-muted-foreground group-hover:text-primary group-hover:bg-muted'
              )}
            >
              <Icon className="size-5" />
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

/**
 * Circular progress ring rendered around the update icon while the
 * background download is running. Total-unknown downloads pulse instead
 * of advancing (mirrors the dialog Progress bar `animate-pulse` fallback).
 */
const UpdateProgressRing: React.FC<{ percent: number | null }> = ({ percent }) => {
  // Radius 11 sits just outside the ArrowUpCircle glyph's visible outline
  // (~8.3px in the 40x40 viewBox), leaving a thin gap so the ring stays
  // legible against the icon while keeping the overall footprint close
  // to the icon's bounding box.
  const radius = 11
  const strokeWidth = 1.5
  const circumference = 2 * Math.PI * radius
  const isIndeterminate = percent === null
  const clamped = isIndeterminate ? 0 : Math.max(0, Math.min(100, percent))
  const offset = circumference * (1 - clamped / 100)

  return (
    <svg
      aria-hidden
      viewBox="0 0 40 40"
      className={cn(
        'absolute inset-0 w-full h-full pointer-events-none',
        isIndeterminate && 'motion-safe:animate-pulse'
      )}
    >
      <circle
        cx="20"
        cy="20"
        r={radius}
        fill="none"
        strokeWidth={strokeWidth}
        className="stroke-amber-500/25 dark:stroke-amber-400/25"
      />
      <circle
        cx="20"
        cy="20"
        r={radius}
        fill="none"
        strokeWidth={strokeWidth}
        strokeLinecap="round"
        strokeDasharray={circumference}
        strokeDashoffset={isIndeterminate ? circumference * 0.65 : offset}
        transform="rotate(-90 20 20)"
        className="stroke-amber-500 dark:stroke-amber-400"
        style={{ transition: isIndeterminate ? undefined : 'stroke-dashoffset 200ms linear' }}
      />
    </svg>
  )
}

interface SidebarProps {
  className?: string
}

const Sidebar: React.FC<SidebarProps> = ({ className }) => {
  const { t } = useTranslation()
  const location = useLocation()
  const navigate = useNavigate()
  const { setting, reloadSetting } = useSetting()
  const [updateDialogOpen, setUpdateDialogOpen] = useState(false)
  const [packageManagerDialogOpen, setPackageManagerDialogOpen] = useState(false)
  const [feedbackOpen, setFeedbackOpen] = useState(false)
  const [disablingDebug, setDisablingDebug] = useState(false)
  const [cancelling, setCancelling] = useState(false)
  /**
   * When the in-app update dialog closes, distinguish "user clicked 稍后"
   * (Cancel button) from other dismissal paths (ESC / outside click / X).
   * Cancel-button onClick sets this to `dialog_later`; onOpenChange(false)
   * reads + clears it, falling back to `dialog_closed` for the other paths.
   */
  const dialogDismissReasonRef = useRef<DismissSource | null>(null)
  const {
    state,
    isCheckingUpdate,
    installUpdate,
    downloadUpdate,
    cancelDownload,
    installKind,
    isManualUpdate,
  } = useUpdate()
  const phase = state.phase

  const isDownloading = phase === 'downloading'
  const isInstalling = phase === 'installing'
  const isReady = phase === 'ready'
  const isAvailable = phase === 'available'
  const indicatorVisible = isAvailable || isDownloading || isReady || isInstalling

  const downloadPercent =
    state.total !== null && state.total > 0
      ? Math.round((state.downloaded / state.total) * 100)
      : null

  const navItems = [
    { to: '/history', icon: Layers, label: t('nav.history') },
    { to: '/devices', icon: Monitor, label: t('nav.devices') },
  ]

  useEffect(() => {
    if (!setting?.general.autoCheckUpdate) {
      setUpdateDialogOpen(false)
    }
  }, [setting?.general.autoCheckUpdate])

  const indicatorLabel = (() => {
    if (isDownloading) {
      return downloadPercent !== null
        ? t('nav.updateDownloadingWithProgress', { percent: downloadPercent })
        : t('nav.updateDownloading')
    }
    if (isInstalling) return t('nav.updateInstalling')
    if (isReady) return t('nav.updateReady')
    return t('nav.updateAvailable')
  })()

  const handlePrimaryAction = async () => {
    if (isInstalling) return
    if (phase === 'idle') return
    captureUpdateActionInvoked('install', 'started')
    try {
      // Both `available` and `ready` go through installUpdate — the backend
      // transparently falls back to `download_and_install` when no cached
      // bytes exist.
      await installUpdate()
      setUpdateDialogOpen(false)
    } catch (error) {
      captureUpdateActionInvoked('install', 'failed')
      log.error({ err: error }, '更新失败')
      toast.error(t('update.installFailed'))
    }
  }

  const handleStartBackgroundDownload = () => {
    captureUpdateActionInvoked('download_bg', 'started')
    setUpdateDialogOpen(false)
    downloadUpdate().catch(error => {
      captureUpdateActionInvoked('download_bg', 'failed')
      log.error({ err: error }, '后台下载失败')
      toast.error(t('update.downloadFailed'))
    })
  }

  const handleCancelDownload = async () => {
    if (!isDownloading || cancelling) return
    setCancelling(true)
    try {
      await cancelDownload()
      captureUpdateActionInvoked('download_bg', 'cancelled')
    } catch (error) {
      log.error({ err: error }, '取消下载失败')
    } finally {
      setCancelling(false)
    }
  }

  const handleIndicatorClick = () => {
    const uiPhase = toUiPhase(phase)
    if (!uiPhase) return
    if (isManualUpdate) {
      captureUpdateDialogOpened('sidebar_icon', uiPhase)
      setPackageManagerDialogOpen(true)
    } else {
      captureUpdateDialogOpened('sidebar_icon', uiPhase)
      setUpdateDialogOpen(true)
    }
  }

  const handleUpdateDialogOpenChange = (open: boolean) => {
    if (!open && updateDialogOpen) {
      const uiPhase = toUiPhase(phase)
      if (uiPhase) {
        const source: DismissSource = dialogDismissReasonRef.current ?? 'dialog_closed'
        captureUpdateDismissed(uiPhase, source)
      }
      dialogDismissReasonRef.current = null
    }
    setUpdateDialogOpen(open)
  }

  const handlePackageManagerDialogOpenChange = (open: boolean) => {
    if (!open && packageManagerDialogOpen) {
      const uiPhase = toUiPhase(phase)
      if (uiPhase) {
        captureUpdateDismissed(uiPhase, 'package_manager_dialog_closed')
      }
    }
    setPackageManagerDialogOpen(open)
  }

  const handleDisableDebugMode = async () => {
    if (disablingDebug) return
    setDisablingDebug(true)
    try {
      await updateDebugMode(false)
      await reloadSetting()
      toast.message(t('debugBadge.disabledToast'))
    } catch (error) {
      log.error({ err: error }, 'Failed to disable debug mode')
      toast.error(t('debugBadge.disableFailed'))
    } finally {
      setDisablingDebug(false)
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
          {setting?.general.debugMode && (
            <TooltipProvider delayDuration={0}>
              <Tooltip>
                <TooltipTrigger asChild>
                  <div className="relative flex size-10 items-center justify-center rounded-lg border border-amber-500/25 bg-amber-500/10 text-amber-700 dark:text-amber-300">
                    <Bug className="size-5" />
                    <button
                      type="button"
                      aria-label={t('debugBadge.disable')}
                      data-tauri-drag-region="false"
                      className="absolute -right-1 -top-1 flex size-4 items-center justify-center rounded-full bg-background text-muted-foreground shadow-sm ring-1 ring-border transition-colors hover:text-foreground"
                      onClick={handleDisableDebugMode}
                      disabled={disablingDebug}
                    >
                      <X className="size-3" />
                    </button>
                  </div>
                </TooltipTrigger>
                <TooltipContent side="right" align="center" className="max-w-64">
                  <div className="space-y-1">
                    <p className="font-medium">{t('debugBadge.title')}</p>
                    <p className="text-xs text-muted-foreground">{t('debugBadge.description')}</p>
                    <p className="text-xs text-muted-foreground">{t('debugBadge.restartHint')}</p>
                  </div>
                </TooltipContent>
              </Tooltip>
            </TooltipProvider>
          )}
          {indicatorVisible && (
            <TooltipProvider delayDuration={0}>
              <Tooltip>
                <TooltipTrigger asChild>
                  <button
                    type="button"
                    aria-label={indicatorLabel}
                    data-update-state={phase}
                    data-tauri-drag-region="false"
                    className="relative group"
                    onClick={handleIndicatorClick}
                    disabled={isCheckingUpdate}
                  >
                    <div
                      className={cn(
                        'relative flex items-center justify-center size-10 rounded-lg transition-colors duration-200 z-10',
                        isReady
                          ? 'text-emerald-600 dark:text-emerald-400 group-hover:bg-muted'
                          : 'text-amber-600 dark:text-amber-400 group-hover:bg-muted'
                      )}
                    >
                      <ArrowUpCircle className="size-5" />

                      {isAvailable && (
                        <span
                          aria-hidden
                          className="absolute top-2.5 right-2.5 flex size-2 motion-reduce:hidden"
                        >
                          <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-amber-500/70 opacity-75" />
                          <span className="relative inline-flex size-2 rounded-full bg-amber-500" />
                        </span>
                      )}
                      {isAvailable && (
                        <span
                          aria-hidden
                          className="hidden motion-reduce:flex absolute top-2.5 right-2.5 size-2 rounded-full bg-amber-500"
                        />
                      )}

                      {(isDownloading || isInstalling) && (
                        <UpdateProgressRing percent={isInstalling ? null : downloadPercent} />
                      )}

                      {isReady && (
                        <span
                          aria-hidden
                          className="absolute -top-0.5 -right-0.5 flex size-3.5 items-center justify-center rounded-full bg-emerald-500 text-white shadow"
                        >
                          <Check className="size-2.5 stroke-[3]" />
                        </span>
                      )}
                    </div>
                  </button>
                </TooltipTrigger>
                <TooltipContent side="right" align="center" className="font-medium">
                  <p>{indicatorLabel}</p>
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
                          'relative flex items-center justify-center size-10 rounded-lg transition-colors duration-200 z-10',
                          'text-muted-foreground group-hover:text-primary group-hover:bg-muted'
                        )}
                      >
                        <MessageSquare className="size-5" />
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
      <AlertDialog open={updateDialogOpen} onOpenChange={handleUpdateDialogOpenChange}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('update.title')}</AlertDialogTitle>
            <AlertDialogDescription asChild>
              <div className="space-y-3">
                <div className="space-y-1 text-sm">
                  <div className="flex items-center justify-between text-muted-foreground">
                    <span>{t('update.currentVersion')}</span>
                    <span className="text-foreground">{state.info?.currentVersion ?? '-'}</span>
                  </div>
                  <div className="flex items-center justify-between text-muted-foreground">
                    <span>{t('update.latestVersion')}</span>
                    <span className="text-foreground">{state.info?.version ?? '-'}</span>
                  </div>
                </div>
                <div className="space-y-2">
                  <div className="text-sm font-medium text-foreground">
                    {t('update.releaseNotes')}
                  </div>
                  <div className="max-h-48 overflow-auto rounded-md border border-border/60 bg-muted/30 px-3 py-2 text-sm text-muted-foreground">
                    <ReleaseNotes content={state.info?.body ?? ''} fallback={t('update.noNotes')} />
                  </div>
                </div>
                {isReady && (
                  <div className="text-xs text-emerald-600 dark:text-emerald-400 pt-1">
                    {t('update.readyHint')}
                  </div>
                )}
                {(isDownloading || isInstalling) && (
                  <div className="space-y-2 pt-2">
                    <div className="flex justify-between text-xs text-muted-foreground">
                      <span>{isInstalling ? t('update.installing') : t('update.downloading')}</span>
                      {downloadPercent !== null && <span>{downloadPercent}%</span>}
                    </div>
                    <Progress
                      value={downloadPercent ?? undefined}
                      className={cn('h-2', downloadPercent === null && 'animate-pulse')}
                    />
                  </div>
                )}
              </div>
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            {isDownloading ? (
              <>
                <AlertDialogCancel
                  onClick={event => {
                    event.preventDefault()
                    void handleCancelDownload()
                  }}
                  disabled={cancelling}
                >
                  {cancelling ? t('update.cancelling') : t('update.cancelDownload')}
                </AlertDialogCancel>
                <AlertDialogAction disabled>{t('update.downloading')}</AlertDialogAction>
              </>
            ) : (
              <>
                <AlertDialogCancel
                  disabled={isInstalling}
                  onClick={() => {
                    dialogDismissReasonRef.current = 'dialog_later'
                  }}
                >
                  {t('update.later')}
                </AlertDialogCancel>
                {isAvailable && (
                  <AlertDialogAction
                    onClick={event => {
                      event.preventDefault()
                      handleStartBackgroundDownload()
                    }}
                    disabled={isInstalling}
                  >
                    {t('update.downloadInBackground')}
                  </AlertDialogAction>
                )}
                <AlertDialogAction
                  onClick={event => {
                    event.preventDefault()
                    void handlePrimaryAction()
                  }}
                  disabled={isInstalling || phase === 'idle'}
                >
                  {isReady ? t('update.installNow') : t('update.updateNow')}
                </AlertDialogAction>
              </>
            )}
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
      {installKind && (
        <PackageManagerUpdateDialog
          open={packageManagerDialogOpen}
          onOpenChange={handlePackageManagerDialogOpenChange}
          installKind={installKind}
          updateInfo={state.info}
        />
      )}
    </>
  )
}

export default Sidebar
