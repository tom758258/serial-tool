import test from 'node:test'
import assert from 'node:assert/strict'
import { LOCALE_STORAGE_KEY, detectBrowserLocale, isSupportedLocale, persistLocale, readSavedLocale, resolveInitialLocale } from '../src/i18n/locale.ts'
import { catalogues, translate } from '../src/i18n/index.ts'

test('saved locale wins over WebView language; unsupported values fall back to detection', () => {
  const saved = { getItem: key => key === LOCALE_STORAGE_KEY ? 'en' : null }
  assert.equal(resolveInitialLocale(saved, { language: 'zh-TW' }), 'en')
  assert.equal(readSavedLocale({ getItem: () => 'zh-CN' }), null)
  assert.equal(resolveInitialLocale({ getItem: () => 'xx' }, { languages: ['zh-Hant-TW', 'en-US'] }), 'zh-TW')
  assert.equal(resolveInitialLocale(null, { language: 'ja-JP' }), 'en')
  assert.equal(detectBrowserLocale({ languages: ['zh-TW', 'en-US'] }), 'zh-TW')
  assert.equal(detectBrowserLocale({ language: 'zh_Hant' }), 'zh-TW')
  assert.equal(detectBrowserLocale({ language: 'zh-CN' }), 'en')
  assert.equal(isSupportedLocale('zh-TW'), true)
  assert.equal(isSupportedLocale('ZH-TW'), false)
})

test('manual language setting is durable when storage works and harmless when blocked', () => {
  let value = ''
  const storage = { getItem: () => value, setItem: (key, next) => { assert.equal(key, LOCALE_STORAGE_KEY); value = next } }
  assert.equal(persistLocale('zh-TW', storage), true)
  assert.equal(resolveInitialLocale(storage, { language: 'en-US' }), 'zh-TW')
  assert.equal(persistLocale('en', { setItem: () => { throw Error('blocked') } }), false)
  assert.equal(readSavedLocale({ getItem: () => { throw Error('blocked') } }), null)
})

test('both language dictionaries have the same nonempty semantic keys', () => {
  const english = Object.keys(catalogues.en).sort()
  assert.deepEqual(Object.keys(catalogues['zh-TW']).sort(), english)
  for (const key of english) {
    assert.match(key, /^(common|connection|terminal|sequence)\.[a-z0-9_.]+$/)
    assert.ok(catalogues.en[key].trim(), key)
    assert.ok(catalogues['zh-TW'][key].trim(), key)
  }
})

test('translation interpolates text, falls back to English and preserves missing-key diagnostics', () => {
  assert.equal(translate('en', 'connection.connect'), 'Connect')
  assert.equal(translate('zh-TW', 'connection.connect'), '連線')
  assert.equal(translate('zh-TW', 'missing.key'), 'missing.key')
  assert.equal(translate('en', 'common.saved_path', { path: '<img onerror=alert(1)>' }), 'Saved <img onerror=alert(1)>')
  const key = 'connection.connect'
  const saved = catalogues['zh-TW'][key]
  delete catalogues['zh-TW'][key]
  try { assert.equal(translate('zh-TW', key), 'Connect') }
  finally { catalogues['zh-TW'][key] = saved }
})
