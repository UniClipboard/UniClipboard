import { getVersion } from '@tauri-apps/api/app'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { SettingSectionHeader } from './SettingSectionHeader'
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
import { Badge } from '@/components/ui/badge'
import { Card, CardContent } from '@/components/ui/card'
import { Progress } from '@/components/ui/progress'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { toast } from '@/components/ui/toast'
import { useSetting } from '@/hooks/useSetting'
import { useUpdate } from '@/hooks/useUpdate'
import { cn } from '@/lib/utils'
import type { UpdateChannel } from '@/types/setting'

function parseChannel(version: string): string {
  const match = version.match(/-(alpha|beta|rc)/)
  return match ? match[1] : 'stable'
}

function getChannelBadgeVariant(channel: string): 'outline' | 'secondary' {
  return channel === 'stable' ? 'secondary' : 'outline'
}

function getChannelLabel(channel: string): string {
  const labels: Record<string, string> = {
    alpha: 'Alpha',
    beta: 'Beta',
    rc: 'RC',
    stable: 'Stable',
  }
  return labels[channel] ?? channel
}

const AboutSection: React.FC = () => {
  const { t } = useTranslation()
  const { setting, loading: settingLoading, updateGeneralSetting } = useSetting()
  const { updateInfo, isCheckingUpdate, checkForUpdates, installUpdate, downloadProgress } =
    useUpdate()
  const [appVersion, setAppVersion] = useState<string>('')
  const [autoCheckUpdate, setAutoCheckUpdate] = useState(true)
  const [updateChannel, setUpdateChannel] = useState<UpdateChannel | null>(null)
  const [updateDialogOpen, setUpdateDialogOpen] = useState(false)
  const [saving, setSaving] = useState(false)
  const isInstallingUpdate = downloadProgress.phase !== 'idle'
  const isBusy = settingLoading || saving

  const channel = appVersion ? parseChannel(appVersion) : null

  useEffect(() => {
    getVersion().then(setAppVersion).catch(console.error)
  }, [])

  useEffect(() => {
    if (!setting?.general) return
    setAutoCheckUpdate(setting.general.auto_check_update)
  }, [setting])

  useEffect(() => {
    if (!setting?.general) return
    setUpdateChannel(setting.general.update_channel ?? null)
  }, [setting])

  const handleAutoCheckUpdateChange = async (checked: boolean) => {
    const previous = autoCheckUpdate
    try {
      setSaving(true)
      setAutoCheckUpdate(checked)
      await updateGeneralSetting({ auto_check_update: checked })
    } catch (error) {
      console.error('更改自动检查更新状态失败:', error)
      setAutoCheckUpdate(previous)
    } finally {
      setSaving(false)
    }
  }

  const handleUpdateChannelChange = async (value: string) => {
    const previous = updateChannel
    try {
      setSaving(true)
      const newChannel = value === 'auto' ? null : (value as UpdateChannel)
      setUpdateChannel(newChannel)
      await updateGeneralSetting({ update_channel: newChannel })
      // Immediately check for updates on the new channel
      checkForUpdates().catch(console.error)
    } catch (error) {
      console.error('更改更新频道失败:', error)
      setUpdateChannel(previous)
    } finally {
      setSaving(false)
    }
  }

  const handleCheckUpdate = async () => {
    try {
      const update = await checkForUpdates()
      if (update) {
        setUpdateDialogOpen(true)
      } else {
        toast.success(t('update.noUpdate'))
      }
    } catch (error) {
      console.error('检查更新失败:', error)
      toast.error(t('update.checkFailed'))
    }
  }

  const handleInstallUpdate = async () => {
    if (!updateInfo || isInstallingUpdate) return
    try {
      await installUpdate()
      setUpdateDialogOpen(false)
    } catch (error) {
      console.error('更新失败:', error)
      toast.error(t('update.installFailed'))
    }
  }

  return (
    <Card>
      <SettingSectionHeader title={t('settings.sections.about.appName')} />
      <CardContent className="pt-0">
        <div className="flex items-center justify-between mb-6">
          <div className="flex items-center">
            <div className="h-12 w-12 rounded-xl bg-gradient-to-br from-primary to-primary/60 flex items-center justify-center shadow-lg shadow-primary/20">
              <svg
                xmlns="http://www.w3.org/2000/svg"
                className="h-7 w-7 text-primary-foreground"
                fill="none"
                viewBox="0 0 24 24"
                stroke="currentColor"
              >
                <title>{t('settings.sections.about.appName')}</title>
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth="2"
                  d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2"
                />
              </svg>
            </div>
            <div className="ml-4 space-y-0.5">
              <div className="flex items-center gap-2">
                <h4 className="text-lg font-medium">{t('settings.sections.about.appName')}</h4>
                {channel && (
                  <Badge variant={getChannelBadgeVariant(channel)}>
                    {getChannelLabel(channel)}
                  </Badge>
                )}
              </div>
              <p className="text-sm text-muted-foreground">
                {appVersion
                  ? t('settings.sections.about.version', { version: appVersion })
                  : t('settings.sections.about.version', { version: '...' })}
              </p>
            </div>
          </div>
          <button
            type="button"
            className="px-4 py-2 bg-secondary hover:bg-secondary/80 text-sm font-medium transition duration-200 rounded-lg"
            onClick={handleCheckUpdate}
            disabled={isBusy || isCheckingUpdate}
          >
            {t('settings.sections.about.checkUpdate')}
          </button>
        </div>

        <div className="flex items-center justify-between py-2">
          <div className="space-y-0.5">
            <h4 className="text-sm font-medium">
              {t('settings.sections.about.autoCheckUpdate.label')}
            </h4>
            <p className="text-xs text-muted-foreground">
              {t('settings.sections.about.autoCheckUpdate.description')}
            </p>
          </div>
          <Switch
            checked={autoCheckUpdate}
            onCheckedChange={handleAutoCheckUpdateChange}
            disabled={isBusy}
          />
        </div>

        <div className="flex items-center justify-between py-2 mt-2">
          <div className="space-y-0.5">
            <h4 className="text-sm font-medium">
              {t('settings.sections.about.updateChannel.label')}
            </h4>
            <p className="text-xs text-muted-foreground">
              {t('settings.sections.about.updateChannel.description')}
            </p>
          </div>
          <Select
            value={updateChannel ?? 'auto'}
            onValueChange={handleUpdateChannelChange}
            disabled={isBusy}
          >
            <SelectTrigger className="w-40">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="auto">
                {t('settings.sections.about.updateChannel.auto')}
              </SelectItem>
              <SelectItem value="stable">
                {t('settings.sections.about.updateChannel.stable')}
              </SelectItem>
              <SelectItem value="alpha">
                {t('settings.sections.about.updateChannel.alpha')}
              </SelectItem>
            </SelectContent>
          </Select>
        </div>

        <div className="space-y-4 pt-4 border-t border-border/50">
          <p className="text-sm text-muted-foreground">{t('settings.sections.about.copyright')}</p>
          <div className="flex space-x-6 text-sm">
            <a
              href="https://github.com/UniClipboard/UniClipboard"
              className="text-primary hover:text-primary/80 transition-colors"
              target="_blank"
              rel="noreferrer"
            >
              {t('settings.sections.about.links.privacyPolicy')}
            </a>
            <a
              href="https://github.com/UniClipboard/UniClipboard"
              className="text-primary hover:text-primary/80 transition-colors"
              target="_blank"
              rel="noreferrer"
            >
              {t('settings.sections.about.links.termsOfService')}
            </a>
            <a
              href="https://github.com/UniClipboard/UniClipboard"
              className="text-primary hover:text-primary/80 transition-colors"
              target="_blank"
              rel="noreferrer"
            >
              {t('settings.sections.about.links.helpCenter')}
            </a>
          </div>
        </div>
      </CardContent>
      <AlertDialog open={updateDialogOpen} onOpenChange={setUpdateDialogOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('update.title')}</AlertDialogTitle>
            <AlertDialogDescription asChild className="space-y-3">
              <div>
                <div className="space-y-1">
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
                  <div className="max-h-48 overflow-auto rounded-md border border-border/60 bg-muted/30 px-3 py-2 text-sm text-muted-foreground whitespace-pre-wrap">
                    {updateInfo?.body?.trim() ? updateInfo.body : t('update.noNotes')}
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
    </Card>
  )
}

export default AboutSection
