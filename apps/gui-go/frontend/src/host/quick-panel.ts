import './install'
import './host.css'
import '@/quick-panel/main'

if (import.meta.env.VITE_GUI_GO_E2E === '1')
  void import('../e2e-secondary').then(m => {
    m.reportMounted('quick-panel', 'quick-panel-mounted')
    m.reportPanelOnShow()
  })
