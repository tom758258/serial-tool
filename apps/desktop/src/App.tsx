import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { invoke, Channel } from '@tauri-apps/api/core'
import { getVersion } from '@tauri-apps/api/app'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { open, save } from '@tauri-apps/plugin-dialog'
import SequenceEditor from './SequenceEditor'
import { useI18n } from './i18n/I18nProvider'
import { continuousRxHex, defaultLine, defaultTcpConnection, display, freshSequence, hex, rxTextFragments, serialSettingDifferences, sequenceMatches, tcpConnectSettings } from './model'
import type { ConnectionSettings, Display, Port, RunResult, Sequence, SessionEvent, TcpConnectionSettings, TerminalDisplay, Transport } from './model'

type Status = 'disconnected' | 'connecting' | 'connected' | 'running' | 'disconnecting' | 'error'
type Theme = 'system' | 'light' | 'dark'
type HistoryEntry = { direction: 'tx' | 'rx'; bytes: number[] }
type Notice = { message: string; kind: 'info' | 'success' | 'error'; localized?: boolean; params?: Record<string, string | number> }
const HISTORY_LIMIT = 5000
function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

function initialTheme(): Theme {
  try {
    const saved = localStorage.getItem('serial-tool.theme')
    return saved === 'light' || saved === 'dark' ? saved : 'system'
  } catch {
    return 'system'
  }
}

function nextTheme(theme: Theme): Theme {
  if (theme === 'system') return 'light'
  if (theme === 'light') return 'dark'
  return 'system'
}

function exportTimestamp(date = new Date()): string {
  const pad = (value: number) => value.toString().padStart(2, '0')
  return `${date.getFullYear()}${pad(date.getMonth() + 1)}${pad(date.getDate())}_${pad(date.getHours())}${pad(date.getMinutes())}${pad(date.getSeconds())}`
}

export default function App() {
  const { locale, toggleLocale, t } = useI18n()
  const [theme, setTheme] = useState<Theme>(initialTheme)
  const [applicationVersion, setApplicationVersion] = useState<string | null>(null)
  const [mode, setMode] = useState<'live' | 'simulation'>('live')
  const [transport, setTransport] = useState<Transport>('serial')
  const [tcpSettings, setTcpSettings] = useState<TcpConnectionSettings>(defaultTcpConnection)
  const [connectionOptionsOpen, setConnectionOptionsOpen] = useState(false)
  const [connectionExpanded, setConnectionExpanded] = useState(true)
  const [exportOpen, setExportOpen] = useState(false)
  const [settings, setSettings] = useState<ConnectionSettings>({ port: '', ...defaultLine })
  const [ports, setPorts] = useState<Port[]>([])
  const [status, setStatus] = useState<Status>('disconnected')
  const [notice, setNotice] = useState<Notice | null>(null)
  const [tab, setTab] = useState<'terminal' | 'sequence'>('terminal')
  const [txFormat, setTxFormat] = useState<'text' | 'hex'>('text')
  const [txInput, setTxInput] = useState('')
  const [autoTxRunning, setAutoTxRunning] = useState(false)
  const [autoTxSource, setAutoTxSource] = useState<'input' | 'file'>('input')
  const [autoTxFilePath, setAutoTxFilePath] = useState<string | null>(null)
  const [txPending, setTxPending] = useState(false)
  const [intervalMs, setIntervalMs] = useState('1000')
  const [rxDisplay, setRxDisplay] = useState<TerminalDisplay>('hex')
  const [showTx, setShowTx] = useState(true)
  const [history, setHistory] = useState<HistoryEntry[]>([])
  const [draft, setDraft] = useState<Sequence>(() => freshSequence(defaultLine))
  const [lastRun, setLastRun] = useState<RunResult | null>(null)
  const [resultDisplay, setResultDisplay] = useState<Display>('hex')
  const terminalEnd = useRef<HTMLDivElement>(null)
  const terminalFollowBottom = useRef(true)
  const resultContentRef = useRef<HTMLDivElement>(null)
  const connectionOptionsWrap = useRef<HTMLDivElement>(null)
  const connectionOptionsToggle = useRef<HTMLButtonElement>(null)
  const currentConnection = useRef(0)

  const connected = status === 'connected' || status === 'running'
  const busy = status === 'connecting' || status === 'disconnecting' || status === 'running'
  const txLocked = busy || autoTxRunning || txPending
  const canTransmit = status === 'connected' && !txLocked
  const serialDifferences = draft.transport.type === 'serial'
    ? serialSettingDifferences(draft.transport.settings, settings) : []
  const transportMatches = sequenceMatches(draft, transport, settings, tcpSettings)
  const autoTxPayloadReady = autoTxSource === 'file' ? Boolean(autoTxFilePath) : Boolean(txInput)
  const settingsLocked = connected || busy
  const hasRxBytes = history.some(entry => entry.direction === 'rx' && entry.bytes.length > 0)
  const isContinuous = rxDisplay === 'continuous-text' || rxDisplay === 'continuous-hex'
  const rxText = useMemo(() => rxTextFragments(history, rxDisplay === 'continuous-text'), [history, rxDisplay])
  const rxHex = useMemo(() => rxDisplay === 'continuous-hex' ? continuousRxHex(history) : '', [history, rxDisplay])
  const nextThemePreference = nextTheme(theme)
  const nextThemeLabel = t('common.theme.' + nextThemePreference)

  useEffect(() => {
    void (async () => {
      try {
        const version = await getVersion()
        if (version.trim()) {
          setApplicationVersion(version)
        }
      } catch {
        // Tauri runtime may be unavailable in browser-only Vite mode.
      }
    })()
  }, [])

  useEffect(() => {
    try { localStorage.setItem('serial-tool.theme', theme) } catch {}
    let media: MediaQueryList | null = null
    if (theme === 'system') {
      try { media = window.matchMedia('(prefers-color-scheme: dark)') } catch {}
    }
    const apply = () => { document.documentElement.dataset.theme = theme === 'system' ? (media?.matches ? 'dark' : 'light') : theme }
    apply()
    try { media?.addEventListener('change', apply) } catch {}
    getCurrentWindow().setTheme(theme === 'system' ? null : theme).catch(error => console.warn('Native theme:', error))
    return () => {
      try { media?.removeEventListener('change', apply) } catch {}
    }
  }, [theme])

  useLayoutEffect(() => {
    if (!terminalFollowBottom.current) return
    const view = terminalEnd.current?.parentElement
    if (view) view.scrollTop = view.scrollHeight
  }, [history, tab, rxDisplay, showTx])

  useLayoutEffect(() => {
    if (resultContentRef.current) resultContentRef.current.scrollTop = 0
  }, [lastRun])

  useEffect(() => {
    if (!connectionOptionsOpen) return
    const dismiss = (event: PointerEvent) => {
      if (!connectionOptionsWrap.current?.contains(event.target as Node | null)) setConnectionOptionsOpen(false)
    }
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return
      setConnectionOptionsOpen(false)
      connectionOptionsToggle.current?.focus()
    }
    document.addEventListener('pointerdown', dismiss)
    document.addEventListener('keydown', closeOnEscape)
    return () => {
      document.removeEventListener('pointerdown', dismiss)
      document.removeEventListener('keydown', closeOnEscape)
    }
  }, [connectionOptionsOpen])

  async function refreshPorts() {
    try {
      setPorts(await invoke<Port[]>('list_serial_ports'))
      setNotice(null)
    } catch (error) { setNotice({ message: errorMessage(error), kind: 'error' }) }
  }

  useEffect(() => { void refreshPorts() }, [])

  function updateSetting(key: keyof ConnectionSettings, value: string) {
    setSettings(previous => ({ ...previous, [key]:
      ['baud_rate', 'data_bits', 'stop_bits', 'timeout_ms'].includes(key) ? Number(value) : value }))
  }

  async function connect() {
    let tcpParameters: ReturnType<typeof tcpConnectSettings> | null = null
    if (transport === 'serial') {
      if (mode === 'live' && !settings.port) { setNotice({ message: 'connection.select_port_first', localized: true, kind: 'error' }); return }
      if (!Number.isInteger(settings.baud_rate) || settings.baud_rate <= 0) { setNotice({ message: 'connection.baud_positive', localized: true, kind: 'error' }); return }
    } else {
      try { tcpParameters = tcpConnectSettings(tcpSettings, mode) }
      catch (error) {
        const raw = errorMessage(error)
        const known: Record<string, string> = {
          'TCP Host must be a nonempty hostname or IP address, not a URL.': 'connection.validation.host',
          'TCP Port must be 1–65535.': 'connection.validation.port_range',
          'TCP Port must be a positive integer.': 'connection.validation.port_integer',
          'Connect timeout (ms) must be a positive integer.': 'connection.validation.connect_integer',
          'I/O timeout (ms) must be a positive integer.': 'connection.validation.io_integer',
        }
        setNotice({ message: known[raw] ?? raw, localized: Boolean(known[raw]), kind: 'error' })
        return
      }
    }
    setAutoTxRunning(false)
    setStatus('connecting')
    setNotice(null)
    const connectionId = ++currentConnection.current
    const channel = new Channel<SessionEvent>()
    channel.onmessage = event => {
      if (connectionId !== currentConnection.current) return
      if (event.kind === 'data') {
        setHistory(previous => [...previous, { direction: event.direction, bytes: event.bytes }].slice(-HISTORY_LIMIT))
      } else if (event.kind === 'connection_error') {
        currentConnection.current++
        setAutoTxRunning(false)
        setNotice({ message: event.message, kind: 'error' })
        setStatus('error')
      } else if (event.kind === 'disconnected') {
        currentConnection.current++
        setAutoTxRunning(false)
        setStatus(previous => previous === 'error' ? 'error' : 'disconnected')
      }
    }
    const previousHistory = history
    terminalFollowBottom.current = true
    setHistory([])
    try {
      await invoke(transport === 'serial' ? 'connect_serial' : 'connect_tcp', {
        mode, settings: transport === 'serial' ? settings : tcpParameters, events: channel,
      })
      setStatus(previous => previous === 'error' || previous === 'disconnected' ? previous : 'connected')
    } catch (error) {
      currentConnection.current++
      setHistory(previousHistory)
      setStatus('error')
      setNotice({ message: errorMessage(error), kind: 'error' })
    }
  }

  async function disconnect() {
    setStatus('disconnecting')
    try {
      await invoke('disconnect_serial')
      currentConnection.current++
      setAutoTxRunning(false)
      setStatus('disconnected')
      setNotice(null)
    } catch (error) {
      setAutoTxRunning(false)
      setStatus('error')
      setNotice({ message: errorMessage(error), kind: 'error' })
    }
  }

  async function send() {
    if (!canTransmit) return
    setTxPending(true)
    try {
      await invoke('send_serial', { input: txInput, format: txFormat })
      setNotice(null)
    } catch (error) { setNotice({ message: errorMessage(error), kind: 'error' }) }
    finally { setTxPending(false) }
  }

  async function toggleAutoTx() {
    if (txPending || status !== 'connected') return
    const interval = Number(intervalMs)
    if (!autoTxRunning && (!Number.isSafeInteger(interval) || interval <= 0)) {
      setNotice({ message: 'terminal.interval_invalid', localized: true, kind: 'error' })
      return
    }
    const connectionId = currentConnection.current
    setTxPending(true)
    try {
      if (autoTxRunning) {
        await invoke('stop_periodic_serial')
        setAutoTxRunning(false)
      } else if (autoTxSource === 'file') {
        if (!autoTxFilePath) return
        await invoke('start_periodic_serial_file', { path: autoTxFilePath, intervalMs: interval })
        if (connectionId === currentConnection.current) setAutoTxRunning(true)
      } else {
        await invoke('start_periodic_serial', { input: txInput, format: txFormat, intervalMs: interval })
        if (connectionId === currentConnection.current) setAutoTxRunning(true)
      }
      setNotice(null)
    } catch (error) { setNotice({ message: errorMessage(error), kind: 'error' }) }
    finally { setTxPending(false) }
  }

  async function chooseAutoTxFile() {
    if (txLocked) return
    try {
      const path = await open({ multiple: false, title: t('terminal.auto_file_dialog') })
      if (path && typeof path === 'string') setAutoTxFilePath(path)
    } catch (error) { setNotice({ message: errorMessage(error), kind: 'error' }) }
  }

  async function sendFile() {
    if (!canTransmit) return
    const connectionId = currentConnection.current
    setTxPending(true)
    try {
      const path = await open({ multiple: false, title: t('terminal.send_file_dialog') })
      if (!path || typeof path !== 'string' || connectionId !== currentConnection.current) return
      await invoke('send_serial_file', { path })
      setNotice({ message: 'terminal.sent_from_path', params: { path }, localized: true, kind: 'success' })
    } catch (error) { setNotice({ message: errorMessage(error), kind: 'error' }) }
    finally { setTxPending(false) }
  }

  async function exportTerminalLog() {
    const content = history.map(entry => `${entry.direction.toUpperCase()} ${hex(entry.bytes)}`).join('\n') + '\n'
    const timestamp = exportTimestamp()
    try {
      const path = await save({
        defaultPath: `terminal-log_${timestamp}.log`,
        filters: [{ name: t('terminal.log_filter'), extensions: ['log'] }],
      })
      if (!path) return
      await invoke('save_text_file', { path, content })
      setNotice({ message: 'common.saved_path', params: { path }, localized: true, kind: 'success' })
    } catch (error) { setNotice({ message: errorMessage(error), kind: 'error' }) }
  }

  async function exportRaw(kind: 'rx' | 'tx-rx') {
    const bytes = history.flatMap(entry => kind === 'tx-rx' || entry.direction === 'rx' ? entry.bytes : [])
    if (!bytes.length) return
    const timestamp = exportTimestamp()
    const defaultPath = kind === 'rx'
      ? `terminal-rx_${timestamp}.bin`
      : `terminal-tx-rx_${timestamp}.bin`
    try {
      const path = await save({
        defaultPath,
        filters: [{ name: t('terminal.raw_filter'), extensions: ['bin'] }],
      })
      if (!path) return
      await invoke('save_binary_file', { path, bytes })
      setNotice({ message: 'common.saved_path', params: { path }, localized: true, kind: 'success' })
    } catch (error) { setNotice({ message: errorMessage(error), kind: 'error' }) }
  }

  async function saveResult() {
    if (!lastRun) return
    const content = JSON.stringify(lastRun, null, 2) + '\n'
    try {
      const path = await save({ defaultPath: 'sequence-result.json', filters: [{ name: t('sequence.result_json'), extensions: ['json'] }] })
      if (!path) return
      await invoke('save_text_file', { path, content })
      setNotice({ message: 'common.saved_path', params: { path }, localized: true, kind: 'success' })
    } catch (error) { setNotice({ message: errorMessage(error), kind: 'error' }) }
  }

  function json(): string { return JSON.stringify(draft) }

  async function validate() {
    try {
      await invoke('validate_sequence', { json: json() })
      setNotice({ message: 'sequence.valid', localized: true, kind: 'success' })
    } catch (error) { setNotice({ message: errorMessage(error), kind: 'error' }) }
  }

  async function loadSequence() {
    try {
      const path = await open({ multiple: false, filters: [{ name: t('sequence.sequence_json'), extensions: ['json'] }] })
      if (!path || typeof path !== 'string') return
      const normalized = await invoke<string>('load_sequence', { path })
      setDraft(JSON.parse(normalized) as Sequence)
      setNotice({ message: 'common.loaded_path', params: { path }, localized: true, kind: 'info' })
    } catch (error) { setNotice({ message: errorMessage(error), kind: 'error' }) }
  }

  async function saveSequence() {
    try {
      await invoke('validate_sequence', { json: json() })
      const path = await save({ defaultPath: 'sequence.json', filters: [{ name: t('sequence.sequence_json'), extensions: ['json'] }] })
      if (!path) return
      await invoke('save_sequence', { path, json: json() })
      setNotice({ message: 'common.saved_path', params: { path }, localized: true, kind: 'info' })
    } catch (error) { setNotice({ message: errorMessage(error), kind: 'error' }) }
  }

  async function runSequence() {
    if (!canTransmit || !transportMatches) return
    setStatus('running')
    setNotice({ message: 'sequence.running', localized: true, kind: 'info' })
    setLastRun(null)
    try {
      const result = await invoke<RunResult>('run_sequence', { json: json() })
      setLastRun(result)
      setNotice(result.status === 'success'
        ? { message: 'sequence.completed', localized: true, kind: 'success' }
        : { message: result.error ?? 'sequence.failed', localized: !result.error, kind: 'error' })
    } catch (error) { setNotice({ message: errorMessage(error), kind: 'error' }) }
    setStatus(previous => previous === 'error' || previous === 'disconnected' ? previous : 'connected')
  }

  return <main>
    <header className="topbar"><div><h1>Serial Tool</h1>{applicationVersion && (
      <span className="subtitle">v{applicationVersion}</span>
    )}</div>
      <div className="header-settings">
        <div className="header-setting"><span>{t('common.appearance')}</span>
          <button type="button" aria-label={t('common.switch_theme', { theme: nextThemeLabel })}
            title={t('common.switch_theme', { theme: nextThemeLabel })}
            onClick={() => setTheme(nextThemePreference)}>◐ {t('common.theme.' + theme)}</button>
        </div>
        <div className="header-setting"><span>{t('common.language')}</span>
          <button type="button" onClick={toggleLocale}
            aria-label={t('common.switch_language', { language: locale === 'en' ? '繁體中文' : 'English' })}
            title={t('common.switch_language', { language: locale === 'en' ? '繁體中文' : 'English' })}>
            <svg viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="currentColor" strokeWidth="1.8" aria-hidden="true">
              <circle cx="12" cy="12" r="9" /><path d="M3 12h18M12 3c-5 5-5 13 0 18M12 3c5 5 5 13 0 18" />
            </svg> <span lang={locale}>{locale === 'en' ? 'English' : '繁體中文'}</span>
          </button>
        </div>
        <div className="header-setting"><span>{t('common.help')}</span>
          <button type="button" onClick={() => {
            void invoke('open_help', { locale, theme }).catch(error =>
              setNotice({ message: 'common.help_open_failed', params: { reason: errorMessage(error) }, localized: true, kind: 'error' }))
          }}>{t('common.help')}</button>
        </div>
      </div>
    </header>

    <section className={`connection panel${connectionExpanded ? '' : ' collapsed'}`}>
      <div className="section-heading"><h2>{t('connection.title')}</h2><div className="connection-header-actions">
        <span className={`status ${status}`}>{t(transport === 'serial' ? 'connection.com_port' : 'connection.tcp')} · {t(mode === 'live' ? 'connection.live' : 'connection.simulation')} · {t('common.status.' + status)}</span>
        <div className="connection-options-wrap" ref={connectionOptionsWrap}>
          <button type="button" ref={connectionOptionsToggle} className="connection-options-toggle" title={t('connection.options')} aria-label={t('connection.options')}
            aria-expanded={connectionOptionsOpen} aria-controls="connection-options" onClick={() => setConnectionOptionsOpen(previous => !previous)}>
            <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" strokeWidth="1.8" aria-hidden="true">
              <path d="m9 3-.7 2.7-2 .9-2.5-.7-2 3.4 1.9 2v2.4l-1.9 2 2 3.4 2.5-.7 2 .9L9 22h4l.7-2.7 2-.9 2.5.7 2-3.4-1.9-2v-2.4l1.9-2-2-3.4-2.5.7-2-.9L13 3Z" />
              <circle cx="11" cy="12.5" r="3" />
            </svg>
          </button>
          <div id="connection-options" className="connection-options" hidden={!connectionOptionsOpen}>
            <h3>{t('connection.options')}</h3>
            <fieldset disabled={settingsLocked} className="execution-mode">
              <legend>{t('connection.execution_mode')}</legend>
              <div className="execution-mode-choices">
                <label><input type="radio" name="execution-mode" value="live" checked={mode === 'live'} onChange={() => setMode('live')} />{t('connection.live')}</label>
                <label><input type="radio" name="execution-mode" value="simulation" checked={mode === 'simulation'} onChange={() => setMode('simulation')} />{t('connection.simulation')}</label>
              </div>
            </fieldset>
            <p className="connection-options-help">{t('connection.simulation_help')}</p>
          </div>
        </div>
        <button type="button" className="connection-collapse-toggle" title={t(connectionExpanded ? 'connection.collapse' : 'connection.expand')}
          aria-label={t(connectionExpanded ? 'connection.collapse' : 'connection.expand')}
          aria-expanded={connectionExpanded} aria-controls="connection-setup-body"
          onClick={() => setConnectionExpanded(previous => !previous)}>{connectionExpanded ? '−' : '+'}</button>
      </div></div>
      <div id="connection-setup-body" hidden={!connectionExpanded}>
      <nav className="transport-tabs" aria-label={t('connection.transport')}>
        <button type="button" disabled={settingsLocked} className={transport === 'serial' ? 'active' : ''} aria-pressed={transport === 'serial'} onClick={() => setTransport('serial')}>{t('connection.com_port')}</button>
        <button type="button" disabled={settingsLocked} className={transport === 'tcp' ? 'active' : ''} aria-pressed={transport === 'tcp'} onClick={() => setTransport('tcp')}>{t('connection.tcp')}</button>
      </nav>
      {transport === 'serial' ? <>
      <div className="connection-resource-row">
        <label>{t('connection.port')}<select disabled={settingsLocked || mode === 'simulation'} value={settings.port} onChange={event => updateSetting('port', event.target.value)}>
          <option value="">{t('connection.select_port')}</option>{ports.map(port => <option key={port.port_name} value={port.port_name}>{port.port_name} · {(['usb', 'pci', 'bluetooth'].includes(port.kind) ? t('connection.port_type.' + port.kind) : t('common.unknown'))}{port.vid != null ? ` ${port.vid.toString(16).padStart(4, '0')}:${port.pid?.toString(16).padStart(4, '0')}` : ''}</option>)}
        </select></label>
        <button disabled={settingsLocked || mode === 'simulation'} onClick={() => void refreshPorts()}>{t('connection.refresh_ports')}</button>
      </div>
      {mode === 'live' && settings.port && <p className="port-detail">{(() => {
        const port = ports.find(value => value.port_name === settings.port)
        return port ? [port.manufacturer, port.product, port.serial_number].filter(Boolean).join(' · ') : ''
      })()}</p>}
      <div className="connection-serial-grid">
        <label>{t('connection.baud_rate')}<input disabled={settingsLocked} type="number" value={settings.baud_rate} onChange={event => updateSetting('baud_rate', event.target.value)} /></label>
        <label>{t('connection.data_bits')}<select disabled={settingsLocked} value={settings.data_bits} onChange={event => updateSetting('data_bits', event.target.value)}>{[5, 6, 7, 8].map(value => <option key={value}>{value}</option>)}</select></label>
        <label>{t('connection.parity')}<select disabled={settingsLocked} value={settings.parity} onChange={event => updateSetting('parity', event.target.value)}>{['none', 'odd', 'even'].map(value => <option key={value} value={value}>{t('common.choice.' + value)}</option>)}</select></label>
        <label>{t('connection.stop_bits')}<select disabled={settingsLocked} value={settings.stop_bits} onChange={event => updateSetting('stop_bits', event.target.value)}>{[1, 2].map(value => <option key={value}>{value}</option>)}</select></label>
        <label>{t('connection.flow_control')}<select disabled={settingsLocked} value={settings.flow_control} onChange={event => updateSetting('flow_control', event.target.value)}>{['none', 'software', 'hardware'].map(value => <option key={value} value={value}>{t('common.choice.' + value)}</option>)}</select></label>
        <label>{t('connection.timeout')}<input disabled={settingsLocked} type="number" value={settings.timeout_ms} onChange={event => updateSetting('timeout_ms', event.target.value)} /></label>
      </div>
      </> : <div className="connection-tcp-grid">
        <label>{t('connection.host')}<input disabled={settingsLocked || mode === 'simulation'} value={tcpSettings.host} placeholder={t('connection.host_placeholder')} onChange={event => setTcpSettings(previous => ({ ...previous, host: event.target.value }))} /></label>
        <label>{t('connection.port')}<input disabled={settingsLocked || mode === 'simulation'} inputMode="numeric" value={tcpSettings.port} placeholder="1–65535" onChange={event => setTcpSettings(previous => ({ ...previous, port: event.target.value }))} /></label>
        <label>{t('connection.connect_timeout')}<input disabled={settingsLocked} inputMode="numeric" value={tcpSettings.connect_timeout_ms} onChange={event => setTcpSettings(previous => ({ ...previous, connect_timeout_ms: event.target.value }))} /></label>
        <label>{t('connection.io_timeout')}<input disabled={settingsLocked} inputMode="numeric" value={tcpSettings.io_timeout_ms} onChange={event => setTcpSettings(previous => ({ ...previous, io_timeout_ms: event.target.value }))} /></label>
      </div>}
      <div className="connection-actions">
        <button className="primary" disabled={settingsLocked || (transport === 'serial' && ((mode === 'live' && !settings.port) || settings.baud_rate <= 0))} onClick={() => void connect()}>{t('connection.connect')}</button>
        <button disabled={status !== 'connected'} onClick={() => void disconnect()}>{t('connection.disconnect')}</button>
      </div>
      </div>
    </section>

    {notice && <div className={`notice notice-${notice.kind}`} role="status">{notice.localized ? t(notice.message, notice.params) : notice.message}</div>}

    <nav className="tabs" aria-label={t('terminal.main_views')}><button className={tab === 'terminal' ? 'active' : ''} onClick={() => { terminalFollowBottom.current = true; setTab('terminal') }}>{t('terminal.title')}</button>
      <button className={tab === 'sequence' ? 'active' : ''} onClick={() => setTab('sequence')}>{t('common.sequence_tab')}</button></nav>

    {tab === 'terminal' ? <section className="terminal panel">
      <div className="section-heading"><h2>{t('terminal.title')}</h2><div className="toolbar"><label>{t('terminal.display_mode')} <select aria-label={t('terminal.display_mode')} value={rxDisplay} onChange={event => { terminalFollowBottom.current = true; setRxDisplay(event.target.value as TerminalDisplay) }}>
        <optgroup label={t('terminal.txrx_events')}>
          <option value="hex">{t('terminal.events_hex')}</option>
          <option value="text">{t('terminal.events_text')}</option>
          <option value="both">{t('terminal.events_both')}</option>
        </optgroup>
        <optgroup label={t('terminal.continuous_rx')}>
          <option value="continuous-text">{t('terminal.continuous_text')}</option>
          <option value="continuous-hex">{t('terminal.continuous_hex')}</option>
        </optgroup>
      </select></label>
        <label className="show-tx"><input type="checkbox" checked={showTx} disabled={isContinuous} onChange={event => { terminalFollowBottom.current = true; setShowTx(event.target.checked) }} />{t('terminal.show_tx')}</label>
        <button onClick={() => { terminalFollowBottom.current = true; setHistory([]); setExportOpen(false) }}>{t('terminal.clear_view')}</button>
        <div className="export-menu-wrap">
          <button type="button" disabled={!history.length} aria-expanded={exportOpen} aria-controls="terminal-export-menu"
            onClick={() => setExportOpen(previous => !previous)}>{t('terminal.export')}</button>
          <div id="terminal-export-menu" className="export-menu" hidden={!exportOpen}>
            <button type="button" onClick={() => { setExportOpen(false); void exportTerminalLog() }}>{t('terminal.export_log')}</button>
            <button type="button" disabled={!hasRxBytes} onClick={() => { setExportOpen(false); void exportRaw('rx') }}>{t('terminal.export_rx')}</button>
            <button type="button" onClick={() => { setExportOpen(false); void exportRaw('tx-rx') }}>{t('terminal.export_txrx')}</button>
          </div>
        </div></div></div>
      <div className="terminal-history" aria-live="polite" onScroll={event => {
        const view = event.currentTarget
        terminalFollowBottom.current = view.scrollHeight - view.scrollTop - view.clientHeight <= 24
      }}>{history.length === 0 && <p className="muted">{t('terminal.empty')}</p>}
        {isContinuous ? <code className={`terminal-continuous ${rxDisplay}`}>{rxDisplay === 'continuous-text' ? rxText.join('') : rxHex}</code> : history.map((entry, index) => (showTx || entry.direction === 'rx') && <div className={`terminal-entry ${entry.direction}`} key={index}>
          <span className="direction">{entry.direction.toUpperCase()}</span><code>{entry.direction === 'tx' ? hex(entry.bytes) :
            rxDisplay === 'hex' ? hex(entry.bytes) :
              rxDisplay === 'text' ? rxText[index] : `${hex(entry.bytes)}  |  ${rxText[index]}`}</code></div>)}
        <div ref={terminalEnd} /></div>
      <div className="send-box"><div className="section-heading"><h3>{t('terminal.send')}</h3><div className="segmented"><button disabled={txLocked} className={txFormat === 'text' ? 'active' : ''} onClick={() => setTxFormat('text')}>{t('common.text')}</button><button disabled={txLocked} className={txFormat === 'hex' ? 'active' : ''} onClick={() => setTxFormat('hex')}>{t('common.hex')}</button></div></div>
        <textarea aria-label={t('terminal.send_data')} value={txInput} onChange={event => setTxInput(event.target.value)} placeholder={txFormat === 'hex' ? '4F 4B 0D 0A' : t('terminal.exact_utf8')} disabled={!connected || txLocked} />
        <div className="toolbar auto-tx-source"><label>{t('terminal.auto_tx_source')} <select aria-label={t('terminal.auto_tx_source')} disabled={txLocked} value={autoTxSource} onChange={event => setAutoTxSource(event.target.value as 'input' | 'file')}>
          <option value="input">{t('terminal.input')}</option><option value="file">{t('terminal.file_raw')}</option></select></label>
          {autoTxSource === 'file' && <><button disabled={txLocked} onClick={() => void chooseAutoTxFile()}>{t('terminal.choose_file')}</button>
            <span className="muted auto-tx-file">{autoTxFilePath ?? t('terminal.no_file')}</span></>}
        </div>
        <div className="send-actions"><span className="muted">{t(txFormat === 'text' ? 'terminal.text_note' : 'terminal.hex_note')} {t('terminal.raw_note')}</span>
          <div className="toolbar"><label>{t('terminal.interval')} <input aria-label={t('terminal.auto_tx_interval')} type="number" min="1" step="1" disabled={!connected || txLocked} value={intervalMs} onChange={event => setIntervalMs(event.target.value)} /> ms</label>
            <button className="primary" disabled={!canTransmit || !txInput} onClick={() => void send()}>{t('terminal.send')}</button>
            <button disabled={!canTransmit} onClick={() => void sendFile()}>{t('terminal.send_file')}</button>
            <button disabled={status !== 'connected' || txPending || (!autoTxRunning && (busy || !autoTxPayloadReady))} onClick={() => void toggleAutoTx()}>{t(autoTxRunning ? 'terminal.stop_auto' : 'terminal.start_auto')}</button></div></div>
      </div>
    </section> : <><SequenceEditor draft={draft} onChange={setDraft} connection={settings} tcpConnection={tcpSettings}
      connectionTransport={transport} disabled={status === 'running'}
      isConnected={connected} serialDifferences={serialDifferences}
      onNew={() => setDraft(freshSequence(settings, transport, Number(tcpSettings.io_timeout_ms)))} onLoad={() => void loadSequence()} onSave={() => void saveSequence()}
      onValidate={() => void validate()} onRun={() => void runSequence()} canRun={canTransmit && transportMatches} />
      <section className="panel results"><div className="section-heading"><h2>{t('sequence.last_run')}</h2><div className="toolbar"><button disabled={!lastRun} onClick={() => void saveResult()}>{t('sequence.save_result')}</button><label>{t('sequence.rx_display')} <select value={resultDisplay} onChange={event => setResultDisplay(event.target.value as Display)}>
        <option value="hex">{t('common.hex')}</option><option value="text">{t('common.text')}</option><option value="both">{t('common.both')}</option></select></label></div></div>
        {lastRun && <p className={`result-status ${lastRun.status}`}>{t('common.status_label')} {t('sequence.run_status.' + lastRun.status)}{lastRun.failing_step_id ? ` · ${lastRun.failing_step_id}` : ''}</p>}
        <div className="result-content" ref={resultContentRef}>
        {!lastRun ? <p className="muted">{t('sequence.no_run')}</p> : <>
          {lastRun.error && <p className="error-text">{lastRun.error}</p>}
          {lastRun.partial_bytes && <p>{t('sequence.partial_bytes')} <code>{display(lastRun.partial_bytes, resultDisplay)}</code></p>}
          <h3>{t('sequence.step_results')}</h3><div className="result-list">{lastRun.step_results.map((result, index) => <div key={index} className="result-row"><strong>{result.step_id}</strong><span>{t('sequence.step.' + result.kind)}</span><code>{result.bytes ? display(result.bytes, resultDisplay) : result.bytes_written != null ? t('sequence.bytes_written', { count: result.bytes_written }) : result.requested_duration_ms != null ? t('sequence.duration_ms', { count: result.requested_duration_ms }) : result.completed_iterations != null ? t('sequence.iterations', { count: result.completed_iterations }) : ''}</code></div>)}</div>
          <h3>{t('sequence.transcript')}</h3><div className="result-list">{lastRun.transcript.map((entry, index) => <div key={index} className="result-row"><strong>{entry.direction.toUpperCase()}</strong><span>{entry.step_id}</span><code>{entry.direction === 'rx' ? display(entry.bytes, resultDisplay) : hex(entry.bytes)}</code></div>)}</div>
        </>}
        </div>
      </section></>}
  </main>
}
