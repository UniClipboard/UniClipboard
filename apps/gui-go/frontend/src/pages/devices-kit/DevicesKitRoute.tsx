import { Navigate } from 'react-router'
import { MANAGE_DEVICES_STATE } from './devices-kit-model'

/**
 * `/devices` in a kit build: Manage Devices is a sheet over the History window (design DV0), so
 * the route opens History with the sheet. Which page a build uses is decided at build time
 * (`@ui-devices-page` in vite.config.ts).
 */
export default function DevicesKitRoute() {
  return <Navigate to="/history" replace state={{ [MANAGE_DEVICES_STATE]: true }} />
}
