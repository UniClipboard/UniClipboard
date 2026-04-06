import { AlertTriangle } from 'lucide-react'
import React, { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { startJoinSpace } from '@/api/daemon/setup'
import {
  AlertDialog,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogMedia,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { Button } from '@/components/ui/button'
import { useSetupRealtimeStore } from '@/store/setupRealtimeStore'

type JoinExistingSpaceDialogProps = {
  open: boolean
  onOpenChange: (open: boolean) => void
}

const JoinExistingSpaceDialog: React.FC<JoinExistingSpaceDialogProps> = ({
  open,
  onOpenChange,
}) => {
  const { t } = useTranslation()
  const { syncSetupStateFromCommand } = useSetupRealtimeStore()
  const [startingJoinSpace, setStartingJoinSpace] = useState(false)

  const handleStartJoinSpace = async () => {
    try {
      setStartingJoinSpace(true)
      const nextState = await startJoinSpace()
      syncSetupStateFromCommand(nextState)
      onOpenChange(false)
    } catch (error) {
      console.error('Failed to start join space flow:', error)
      toast.error(t('devices.joinSpaceDialog.errors.startFailed'))
    } finally {
      setStartingJoinSpace(false)
    }
  }

  return (
    <AlertDialog
      open={open}
      onOpenChange={nextOpen => {
        if (!startingJoinSpace) {
          onOpenChange(nextOpen)
        }
      }}
    >
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogMedia className="bg-amber-500/10 text-amber-600 dark:text-amber-400">
            <AlertTriangle className="h-5 w-5" />
          </AlertDialogMedia>
          <AlertDialogTitle>{t('devices.joinSpaceDialog.title')}</AlertDialogTitle>
          <AlertDialogDescription asChild>
            <div className="space-y-3 text-left">
              <p>{t('devices.joinSpaceDialog.description')}</p>
              <ul className="list-disc space-y-1 pl-5">
                <li>{t('devices.joinSpaceDialog.points.notMerge')}</li>
                <li>{t('devices.joinSpaceDialog.points.joinSelectedDevicesSpace')}</li>
                <li>{t('devices.joinSpaceDialog.points.localEncryptedContentUnavailable')}</li>
              </ul>
              <p>{t('devices.joinSpaceDialog.backupHint')}</p>
            </div>
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <Button
            variant="outline"
            onClick={() => onOpenChange(false)}
            disabled={startingJoinSpace}
          >
            {t('devices.joinSpaceDialog.actions.cancel')}
          </Button>
          <Button
            variant="destructive"
            onClick={() => void handleStartJoinSpace()}
            disabled={startingJoinSpace}
          >
            {startingJoinSpace
              ? t('devices.joinSpaceDialog.actions.starting')
              : t('devices.joinSpaceDialog.actions.confirm')}
          </Button>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}

export default JoinExistingSpaceDialog
