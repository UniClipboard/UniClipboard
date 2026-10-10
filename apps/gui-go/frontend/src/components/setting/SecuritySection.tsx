import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import ChangePassphraseDialog from '@/components/security/ChangePassphraseDialog'
import {
  getPassphraseChangeAvailability,
  getPassphraseChangeAvailabilityMessageKey,
} from '@/components/security/passphrase-change-availability'
import { Button, Switch } from '@/components/ui'
import { toast } from '@/components/ui/toast'
import { useDeviceTrust } from '@/hooks/useDeviceTrust'
import { useSetting } from '@/hooks/useSetting'
import { SettingGroup } from './SettingGroup'
import { SettingRow } from './SettingRow'
import { useOptimisticSetting } from './useOptimisticSetting'

const SecuritySection: React.FC = () => {
  const { t } = useTranslation()
  const { setting, error, updateSecuritySetting } = useSetting()
  const { snapshot, loading: deviceTrustLoading } = useDeviceTrust()
  const [changePassphraseOpen, setChangePassphraseOpen] = useState(false)
  const passphraseChangeAvailability = getPassphraseChangeAvailability(snapshot, deviceTrustLoading)
  const passphraseChangeDescription = t(
    getPassphraseChangeAvailabilityMessageKey(passphraseChangeAvailability)
  )

  const [autoUnlockEnabled, setAutoUnlockEnabled] = useOptimisticSetting(
    setting?.security.autoUnlockEnabled ?? false,
    next => updateSecuritySetting({ autoUnlockEnabled: next }),
    { failureLog: 'Failed to change auto-unlock setting' }
  )

  // Display error message if there is an error
  if (error) {
    return (
      <div className="text-red-500 py-4">
        {t('settings.sections.security.loadError')}: {error}
      </div>
    )
  }

  return (
    <>
      <SettingGroup>
        <SettingRow
          label={t('settings.sections.security.autoUnlock.label')}
          description={t('settings.sections.security.autoUnlock.description')}
        >
          <Switch checked={autoUnlockEnabled} onCheckedChange={setAutoUnlockEnabled} />
        </SettingRow>
        <SettingRow
          label={t('passphraseChange.entry.label')}
          description={passphraseChangeDescription}
        >
          <Button
            variant="outline"
            onClick={() => setChangePassphraseOpen(true)}
            disabled={passphraseChangeAvailability !== 'available'}
          >
            {t('passphraseChange.entry.button')}
          </Button>
        </SettingRow>
      </SettingGroup>
      <ChangePassphraseDialog
        open={changePassphraseOpen}
        onOpenChange={setChangePassphraseOpen}
        onChanged={() => {
          setChangePassphraseOpen(false)
          toast.success(t('passphraseChange.success'))
        }}
      />
    </>
  )
}

export default SecuritySection
