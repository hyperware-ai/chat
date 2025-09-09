// Entry point for the React application
import React from 'react'
import ReactDOM from 'react-dom/client'
import App from './App.tsx'
import './index.css'
// Auto-start hw:// protocol link handling
import '@hyperware-ai/hw-protocol-watcher'

// Create root and render the app
ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
)