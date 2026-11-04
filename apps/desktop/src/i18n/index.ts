import en from './en/index.ts'
import zhTW from './zh-TW/index.ts'
import type { Locale } from './locale.ts'

export type TranslationParams = Record<string, string | number>
export const catalogues: Record<Locale, Record<string, string>> = { en, 'zh-TW': zhTW }

export function translate(locale: Locale, key: string, params: TranslationParams = {}): string {
  const template = catalogues[locale][key] ?? catalogues.en[key] ?? key
  return template.replace(/\{([a-zA-Z][a-zA-Z0-9_]*)\}/g, (match, name: string) =>
    Object.prototype.hasOwnProperty.call(params, name) ? String(params[name]) : match)
}
