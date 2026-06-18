import { DiagnosticsSettings } from './general/DiagnosticsSettings'
import { LanguageSettings } from './general/LanguageSettings'
import { StartupSettings } from './general/StartupSettings'
import { TelemetrySettings } from './general/TelemetrySettings'

export default function GeneralSection() {
  return (
    <>
      <StartupSettings />
      <LanguageSettings />
      <TelemetrySettings />
      <DiagnosticsSettings />
    </>
  )
}
