import React from 'react'
import ReactDOM from 'react-dom/client'
import QuickPanelApp from './QuickPanelApp'
import '@/i18n'
import { initializeUiScale } from '@/lib/ui-scale'
import '@/styles/globals.css'

initializeUiScale()

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <QuickPanelApp />
  </React.StrictMode>
)
