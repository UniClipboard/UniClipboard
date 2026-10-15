import { useEffect, useEffectEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { InlineNotice } from '@/ui-kit'

// The joined message stays as long as the old success step did before closing.
const JOINED_NOTICE_MS = 5000

/** "Device joined" on the detail of the device an invitation just added; it goes away by itself. */
export function DevicesKitJoinedNotice({ onDismiss }: { onDismiss: () => void }) {
  const { t } = useTranslation()
  const dismiss = useEffectEvent(onDismiss)
  useEffect(() => {
    const id = setTimeout(() => dismiss(), JOINED_NOTICE_MS)
    return () => clearTimeout(id)
  }, [])
  return (
    <div data-testid="add-device-success">
      <InlineNotice
        tone="success"
        title={t('devices.addDevice.success.title')}
        text={t('devices.addDevice.success.subtitle')}
        dismiss={{ label: t('devices.addDevice.actions.close'), onClick: onDismiss }}
      />
    </div>
  )
}
