import { useTranslation } from 'react-i18next'
import { AllowOverlayAddrsDisclosure } from '@/components/setting/AllowOverlayAddrsDisclosure'
import { FixedNetworkPortField } from '@/components/setting/FixedNetworkPortField'
import { LanOnlyDisclosure } from '@/components/setting/LanOnlyDisclosure'
import { SettingSwitch } from '@/components/setting/SettingControls'
import { SettingGroup } from '@/components/setting/SettingGroup'
import { SettingRow } from '@/components/setting/SettingRow'

interface NetworkConnectionSettingsProps {
  lanOnly: boolean
  allowOverlayNetworkAddrs: boolean
  bindPort: number
  onLanOnlyChange: (checked: boolean) => void
  onAllowOverlayChange: (checked: boolean) => void
  onBindPortChange: (port: number | undefined) => void
}

export function NetworkConnectionSettings({
  lanOnly,
  allowOverlayNetworkAddrs,
  bindPort,
  onLanOnlyChange,
  onAllowOverlayChange,
  onBindPortChange,
}: NetworkConnectionSettingsProps) {
  const { t } = useTranslation()
  return (
    <SettingGroup title={t('settings.sections.network.groups.connection')}>
      <FixedNetworkPortField value={bindPort} onChange={onBindPortChange} />
      <SettingRow
        label={t('settings.sections.network.lanOnly.label')}
        labelExtra={<LanOnlyDisclosure />}
        description={t('settings.sections.network.lanOnly.description')}
        experimentalKey="network.lanOnly"
      >
        <SettingSwitch
          id="lan-only-switch"
          label={t('settings.sections.network.lanOnly.label')}
          checked={lanOnly}
          onCheckedChange={onLanOnlyChange}
        />
      </SettingRow>
      <SettingRow
        label={t('settings.sections.network.allowOverlayAddrs.label')}
        labelExtra={<AllowOverlayAddrsDisclosure />}
        description={t('settings.sections.network.allowOverlayAddrs.description')}
        experimentalKey="network.allowOverlayAddrs"
      >
        <SettingSwitch
          id="allow-overlay-addrs-switch"
          label={t('settings.sections.network.allowOverlayAddrs.label')}
          checked={allowOverlayNetworkAddrs}
          onCheckedChange={onAllowOverlayChange}
        />
      </SettingRow>
    </SettingGroup>
  )
}
