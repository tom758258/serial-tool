import { createContext, useContext, useEffect, useState } from 'react'
import type { ReactNode } from 'react'
import { translate } from './index.ts'
import type { TranslationParams } from './index.ts'
import { persistLocale } from './locale.ts'
import type { Locale } from './locale.ts'

type I18nContextValue = { locale: Locale; toggleLocale: () => void; t: (key: string, params?: TranslationParams) => string }
const I18nContext = createContext<I18nContextValue | null>(null)

export function I18nProvider({ children, initialLocale }: { children: ReactNode; initialLocale: Locale }) {
  const [locale, setLocale] = useState<Locale>(initialLocale)
  useEffect(() => { document.documentElement.lang = locale }, [locale])
  const toggleLocale = () => {
    const next = locale === 'en' ? 'zh-TW' : 'en'
    setLocale(next)
    try { persistLocale(next, window.localStorage) } catch { /* Keep in-memory selection. */ }
  }
  const t = (key: string, params?: TranslationParams) => translate(locale, key, params)
  return <I18nContext.Provider value={{ locale, toggleLocale, t }}>{children}</I18nContext.Provider>
}

export function useI18n(): I18nContextValue {
  const value = useContext(I18nContext)
  if (!value) throw new Error('I18nProvider is missing')
  return value
}
