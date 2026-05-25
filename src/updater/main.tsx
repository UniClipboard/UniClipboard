import React from 'react'
import ReactDOM from 'react-dom/client'
import '@/i18n'
import { initializeWindowUi } from '@/lib/window-ui'
import '@/styles/globals.css'
import UpdaterWindow from './UpdaterWindow'

initializeWindowUi()

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <UpdaterWindow />
  </React.StrictMode>
)
