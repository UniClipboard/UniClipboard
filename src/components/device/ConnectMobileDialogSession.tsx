import { useCallback, useId, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type {
  MobileSyncSettingsView,
  RegisterMobileDeviceResult,
} from '@/api/tauri-command/mobile_sync'
import { AddDeviceDialogBody } from '@/components/device/AddDeviceDialogBody'
import DeviceInvitationActions from '@/components/device/DeviceInvitationActions'
import MobileRegularSyncFlow from '@/components/device/MobileRegularSyncFlow'
import { Badge } from '@/components/ui/badge'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { useAddDeviceInvitation } from '@/hooks/useAddDeviceInvitation'

export interface ConnectMobileDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  onMobileSuccess: (result: RegisterMobileDeviceResult) => void
  onDirectSuccess: () => void
  onSettingsChange: (settings: MobileSyncSettingsView) => void
  onConfigure: () => void
}

export default function ConnectMobileDialogSession({
  open,
  onOpenChange,
  onMobileSuccess,
  onDirectSuccess,
  onSettingsChange,
  onConfigure,
  onOpenChangeComplete,
}: ConnectMobileDialogProps & { onOpenChangeComplete: (open: boolean) => void }) {
  const { t } = useTranslation()
  const [mode, setMode] = useState('regular')
  const [directStarted, setDirectStarted] = useState(false)
  const [regularBusy, setBusy] = useState(false)
  const [footerContainer, setFooterContainer] = useState<HTMLDivElement | null>(null)
  const directFormId = useId()
  const invitationState = useAddDeviceInvitation({
    open: open && directStarted,
    active: open && mode === 'direct',
    onOpenChange,
    onSuccess: onDirectSuccess,
  })
  const busy = regularBusy || invitationState.loading
  const close = useCallback(() => onOpenChange(false), [onOpenChange])
  return (
    <Dialog
      open={open}
      onOpenChange={next => {
        if (!busy) onOpenChange(next)
      }}
      onOpenChangeComplete={onOpenChangeComplete}
      disablePointerDismissal
    >
      <DialogContent className="overflow-hidden sm:max-w-md" showCloseButton={!busy}>
        <DialogHeader>
          <DialogTitle>{t('devices.connectMobile.title')}</DialogTitle>
        </DialogHeader>
        <Tabs
          value={mode}
          onValueChange={value => {
            if (busy) return
            setMode(String(value))
            if (value === 'direct') setDirectStarted(true)
          }}
          className="min-h-0 gap-4"
        >
          <TabsList
            className="h-auto! w-full items-stretch"
            aria-label={t('devices.connectMobile.method')}
          >
            <TabsTrigger
              value="regular"
              disabled={busy}
              className="h-auto min-h-8 whitespace-normal"
            >
              {t('devices.connectMobile.regular')}
            </TabsTrigger>
            <TabsTrigger
              value="direct"
              disabled={busy}
              className="h-auto min-h-8 flex-wrap whitespace-normal"
            >
              {t('devices.connectMobile.direct')}
              <Badge variant="outline" className="text-muted-foreground">
                {t('devices.connectMobile.experimental')}
              </Badge>
            </TabsTrigger>
          </TabsList>
          <DialogDescription className="min-h-10">
            {t(`devices.connectMobile.${mode}Description`)}
          </DialogDescription>
          <TabsContent
            value="regular"
            keepMounted
            className="-mx-1 min-h-0 overflow-y-auto px-1 py-1 data-[hidden]:hidden"
          >
            <MobileRegularSyncFlow
              onCancel={close}
              onBusyChange={setBusy}
              onSettingsChange={onSettingsChange}
              onConfigure={onConfigure}
              footerContainer={mode === 'regular' ? footerContainer : null}
              onSuccess={result => {
                onMobileSuccess(result)
                close()
              }}
            />
          </TabsContent>
          <TabsContent
            value="direct"
            keepMounted
            className="-mx-1 min-h-0 overflow-y-auto px-1 py-1 data-[hidden]:hidden"
          >
            {directStarted && (
              <AddDeviceDialogBody invitationState={invitationState} formId={directFormId} />
            )}
          </TabsContent>
        </Tabs>
        <DialogFooter ref={setFooterContainer}>
          {mode === 'direct' && (
            <DeviceInvitationActions
              invitationState={invitationState}
              onOpenChange={onOpenChange}
              formId={directFormId}
            />
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
