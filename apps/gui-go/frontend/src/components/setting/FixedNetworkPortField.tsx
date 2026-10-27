import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { SettingTextField } from '@/components/setting/SettingControls'
import { SettingRow } from '@/components/setting/SettingRow'

interface FixedNetworkPortFieldProps {
  value: number
  onChange: (port: number | undefined) => void
}

export function FixedNetworkPortField({ value, onChange }: FixedNetworkPortFieldProps) {
  const { t } = useTranslation()
  const [invalidInput, setInvalidInput] = useState<string | null>(null)

  const handleChange = (input: string) => {
    const port = input === '' ? 0 : Number(input)
    const valid =
      input === '' ||
      (/^[0-9]+$/.test(input) && Number.isInteger(port) && port >= 1 && port <= 65535)
    setInvalidInput(valid ? null : input)
    onChange(valid ? port : undefined)
  }

  return (
    <SettingRow
      label={t('settings.sections.network.bindPort.label')}
      description={t('settings.sections.network.bindPort.description')}
    >
      <div className="flex w-56 min-w-0">
        <SettingTextField
          label={t('settings.sections.network.bindPort.label')}
          value={invalidInput ?? (value === 0 ? '' : String(value))}
          onValueChange={handleChange}
          placeholder={t('settings.sections.network.bindPort.placeholder')}
          error={
            invalidInput === null ? undefined : t('settings.sections.network.bindPort.invalid')
          }
        />
      </div>
    </SettingRow>
  )
}
