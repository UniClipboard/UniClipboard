import React from 'react'
import ReactDOM from 'react-dom/client'
import ClipboardHistoryPanel from './ClipboardHistoryPanel'
import '@/styles/globals.css'

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <ClipboardHistoryPanel />
  </React.StrictMode>
)
