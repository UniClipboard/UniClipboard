import DevicesPage from '@ui-devices-page'
import HistoryPage from '@ui-history-page'
import SettingsPage, { settingsOwnsWindowTop } from '@ui-settings-page'
import type { ReactNode } from 'react'
import { Navigate, Route } from 'react-router'
import { DeviceTrustDialogHost } from '@/components/device/DeviceTrustDialogHost'
import { GlobalShortcuts } from '@/components/GlobalShortcuts'
import StartupModals from '@/components/StartupModals'
import { Toaster } from '@/components/ui/toaster'
import { DeviceTrustProvider } from '@/contexts/DeviceTrustContext'
import { SettingsFullLayout } from '@/layouts'
import { DiagnosticsRoutes } from '@/observability/diagnostics'
import { AuthenticatedLayout } from './AuthenticatedLayout'

type AuthenticatedRoutesProps = { fullTitleBar: ReactNode }

export function AuthenticatedRoutes({ fullTitleBar }: AuthenticatedRoutesProps) {
  return (
    <DeviceTrustProvider enabled>
      <GlobalShortcuts />
      <DiagnosticsRoutes>
        <Route element={<AuthenticatedLayout />}>
          <Route path="/" element={<Navigate to="/history" replace />} />
          <Route path="/history" element={<HistoryPage />} />
          <Route path="/devices" element={<DevicesPage />} />
        </Route>
        <Route
          element={<SettingsFullLayout titleBar={settingsOwnsWindowTop ? null : fullTitleBar} />}
        >
          <Route path="/settings" element={<SettingsPage />} />
        </Route>
        <Route path="*" element={<Navigate to="/" replace />} />
      </DiagnosticsRoutes>
      <Toaster />
      <StartupModals />
      <DeviceTrustDialogHost />
    </DeviceTrustProvider>
  )
}
