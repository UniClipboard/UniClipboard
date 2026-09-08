import type { ReactNode } from 'react'
import { Navigate, Route } from 'react-router'
import { DeviceTrustDialogHost } from '@/components/device/DeviceTrustDialogHost'
import { GlobalShortcuts } from '@/components/GlobalShortcuts'
import StartupModals from '@/components/StartupModals'
import { Toaster } from '@/components/ui/toaster'
import { DeviceTrustProvider } from '@/contexts/DeviceTrustContext'
import { SettingsFullLayout } from '@/layouts'
import { DiagnosticsRoutes } from '@/observability/diagnostics'
import DevicesPage from '@/pages/DevicesPage'
import HistoryPage from '@/pages/HistoryPage'
import SettingsPage from '@/pages/SettingsPage'
import { AuthenticatedLayout } from './AuthenticatedLayout'

type AuthenticatedRoutesProps = { fullTitleBar: ReactNode; sidebarTitle: ReactNode }

export function AuthenticatedRoutes({ fullTitleBar, sidebarTitle }: AuthenticatedRoutesProps) {
  return (
    <DeviceTrustProvider enabled>
      <GlobalShortcuts />
      <DiagnosticsRoutes>
        <Route element={<AuthenticatedLayout sidebarTitle={sidebarTitle} />}>
          <Route path="/" element={<Navigate to="/history" replace />} />
          <Route path="/history" element={<HistoryPage />} />
          <Route path="/devices" element={<DevicesPage />} />
        </Route>
        <Route element={<SettingsFullLayout titleBar={fullTitleBar} />}>
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
