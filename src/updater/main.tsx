import { LazyMotion, domMax } from 'framer-motion'
import React from 'react'
import ReactDOM from 'react-dom/client'
import '@/i18n'
import { initializeUiSound } from '@/lib/ui-sound'
import { initializeWindowUi } from '@/lib/window-ui'
import '@/styles/globals.css'
import UpdaterWindow from './UpdaterWindow'

initializeWindowUi()
initializeUiSound()

// This window has its own React root (separate from App.tsx), so it needs its
// own LazyMotion provider for the `m` components used inside (e.g. <Switch>).
// domMax matches App.tsx and supplies the layout animation feature the switch
// thumb relies on.
ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <LazyMotion features={domMax} strict>
      <UpdaterWindow />
    </LazyMotion>
  </React.StrictMode>
)
