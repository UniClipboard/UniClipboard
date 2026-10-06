import './host/install'
import './host/host.css'
import '@/updater/main'

if (import.meta.env.VITE_GUI_GO_E2E === '1')
  void import('./e2e-secondary').then(m => m.reportMounted('updater', 'updater-mounted', '0.99.0'))
