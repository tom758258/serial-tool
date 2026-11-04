export type Locale = 'en' | 'zh-TW'
export const LOCALE_STORAGE_KEY = 'serial-tool.locale'

export function isSupportedLocale(value: unknown): value is Locale {
  return value === 'en' || value === 'zh-TW'
}

export function detectBrowserLocale(navigatorLike?: { languages?: readonly string[]; language?: string } | null): Locale {
  let language = ''
  try {
    language = navigatorLike?.languages?.find(value => typeof value === 'string' && value.trim()) || navigatorLike?.language || ''
  } catch { /* Language detection is optional. */ }
  const normalized = language.trim().replaceAll('_', '-').toLowerCase()
  return normalized === 'zh-tw' || normalized.startsWith('zh-tw-') ||
    normalized === 'zh-hant' || normalized.startsWith('zh-hant-') ? 'zh-TW' : 'en'
}

export function readSavedLocale(storage?: Pick<Storage, 'getItem'> | null): Locale | null {
  try {
    const saved = storage?.getItem(LOCALE_STORAGE_KEY)
    return isSupportedLocale(saved) ? saved : null
  } catch { return null }
}

export function resolveInitialLocale(storage?: Pick<Storage, 'getItem'> | null,
  navigatorLike?: { languages?: readonly string[]; language?: string } | null): Locale {
  return readSavedLocale(storage) ?? detectBrowserLocale(navigatorLike)
}

export function persistLocale(locale: Locale, storage?: Pick<Storage, 'setItem'> | null): boolean {
  try {
    if (!storage) return false
    storage.setItem(LOCALE_STORAGE_KEY, locale)
    return true
  } catch { return false }
}
