import './install'
// Reads the `window-material` launch parameter before the router rewrites the URL (`/` -> `/history`).
import './window-material'
import './host.css'
import '@/main'

if (import.meta.env.VITE_GUI_GO_E2E === '1') void import('../e2e-driver')
