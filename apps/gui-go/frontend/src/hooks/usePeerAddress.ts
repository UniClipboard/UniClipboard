import { useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { DaemonApiError } from '@/api/daemon/errors'
import type { PeerAddressDto } from '@/api/generated/types.gen'
import { validatePeerAddress } from '@/components/device/peer-address-utils'
import { useSettingSelector } from '@/hooks/useSetting'
import { trackNetworkSettingsSave } from '@/store/networkRestartStore'

/** Both device views edit one device's discovery hint through the settings authority. */
export function usePeerAddress(deviceId: string) {
  const { t } = useTranslation()
  const saved = useSettingSelector(({ setting }) => setting?.network.peerAddresses?.[deviceId])
  const loading = useSettingSelector(({ loading }) => loading)
  const loadError = useSettingSelector(({ error }) => error)
  const updateNetworkSetting = useSettingSelector(context => context.updateNetworkSetting)
  const [draft, setDraft] = useState<{ ip: string; port: string } | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const saveInFlightRef = useRef<Promise<void> | null>(null)
  const values = draft ?? { ip: saved?.ip ?? '', port: saved ? String(saved.port) : '' }
  const validation = validatePeerAddress(values.ip, values.port)
  const ipError =
    draft && !validation.valid && validation.field === 'ip'
      ? t('devices.directAddress.invalidIp')
      : undefined
  const portError =
    draft && !validation.valid && validation.field === 'port'
      ? t('devices.directAddress.invalidPort')
      : undefined

  const edit = (field: 'ip' | 'port', value: string) => {
    setDraft({ ...values, [field]: value })
    setError(null)
  }
  const persist = (address: PeerAddressDto | null): Promise<void> => {
    if (saveInFlightRef.current) return saveInFlightRef.current
    setBusy(true)
    setError(null)
    const save = trackNetworkSettingsSave(
      updateNetworkSetting({ peerAddresses: { [deviceId]: address } })
    )
      .then(() => {
        setDraft(null)
      })
      .catch((reason: unknown) => {
        const message =
          reason instanceof DaemonApiError
            ? (reason.apiMessage ?? reason.message)
            : reason instanceof Error
              ? reason.message
              : t('devices.directAddress.saveError')
        setError(message)
      })
      .finally(() => {
        if (saveInFlightRef.current === save) saveInFlightRef.current = null
        setBusy(false)
      })
    saveInFlightRef.current = save
    return save
  }

  return {
    ...values,
    busy,
    error: loadError ? t('devices.directAddress.loadError') : error,
    ipError,
    portError,
    disabled: loading || !!loadError || busy,
    canSave: validation.valid && draft !== null,
    canClear: saved !== undefined || draft !== null,
    setIp: (value: string) => edit('ip', value),
    setPort: (value: string) => edit('port', value),
    save: () => (validation.valid ? persist(validation.address) : Promise.resolve()),
    clear: () => persist(null),
  }
}
