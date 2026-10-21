import { DeviceNameSettings } from '@/components/setting/general/DeviceNameSettings'
import { DiagnosticsSettings } from './general/DiagnosticsSettings'
import { LanguageSettings } from './general/LanguageSettings'
import { SoundSettings } from './general/SoundSettings'
import { StartupSettings } from './general/StartupSettings'
import { TelemetrySettings } from './general/TelemetrySettings'
import { TrayIconSettings } from './general/TrayIconSettings'

export default function GeneralSection() {
  return (
    <>
      <DeviceNameSettings />
      <StartupSettings />
      <TrayIconSettings />
      <LanguageSettings />
      <SoundSettings />
      <TelemetrySettings />
      <DiagnosticsSettings />
    </>
  )
}
