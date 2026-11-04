import React from 'react'
import { createRoot } from 'react-dom/client'
import App from './App'
import { I18nProvider } from './i18n/I18nProvider'
import { resolveInitialLocale } from './i18n/locale'
import './styles.css'

let storage: Storage | undefined
try { storage = window.localStorage } catch { /* Storage can be unavailable. */ }
const initialLocale = resolveInitialLocale(storage, navigator)
document.documentElement.lang = initialLocale

createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <I18nProvider initialLocale={initialLocale}>
      <App />
    </I18nProvider>
  </React.StrictMode>,
)
