import './host/install'
import './host/host.css'
import '@/main'

if (import.meta.env.VITE_GUI_GO_E2E === '1') void import('./e2e-driver')
