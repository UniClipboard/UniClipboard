import React from 'react'
import ReactDOM from 'react-dom/client'
import PreviewPanel from './PreviewPanel'
import '@/i18n'
import '@/styles/globals.css'

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <PreviewPanel />
  </React.StrictMode>
)
