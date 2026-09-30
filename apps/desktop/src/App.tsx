import { useEffect, useMemo, useRef, useState } from 'react'
import { invoke, Channel } from '@tauri-apps/api/core'
import { getVersion } from '@tauri-apps/api/app'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { open, save } from '@tauri-apps/plugin-dialog'
import SequenceEditor from './SequenceEditor'
import { continuousRxHex, defaultLine, display, freshSequence, hex, rxTextFragments, serialSettingDifferences } from './model'
import type { ConnectionSettings, Display, Port, RunResult, Sequence, SessionEvent, TerminalDisplay } from './model'

type Status = 'disconnected' | 'connecting' | 'connected' | 'running' | 'disconnecting' | 'error'
type Theme = 'system' | 'light' | 'dark'
type HistoryEntry = { direction: 'tx' | 'rx'; bytes: number[] }
type Notice = { message: string; kind: 'info' | 'success' | 'error' }
const HISTORY_LIMIT = 5000
const statusLabels: Record<Status, string> = {
  disconnected: 'Disconnected', connecting: 'Connecting…', connected: 'Connected',
  running: 'Running…', disconnecting: 'Disconnecting…', error: 'Error',
}

function portTypeLabel(kind: string): string {
  switch (kind) {
    case 'usb': return 'USB'
    case 'pci': return 'PCI'
    case 'bluetooth': return 'Bluetooth'
    default: return 'Unknown'
  }
}

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

function themeLabel(theme: Theme): string {
  return theme[0].toUpperCase() + theme.slice(1)
}

function exportTimestamp(date = new Date()): string {
  const pad = (value: number) => value.toString().padStart(2, '0')
  return `${date.getFullYear()}${pad(date.getMonth() + 1)}${pad(date.getDate())}_${pad(date.getHours())}${pad(date.getMinutes())}${pad(date.getSeconds())}`
}

export default function App() {
  const [theme, setTheme] = useState<Theme>(initialTheme)
  const [applicationVersion, setApplicationVersion] = useState<string | null>(null)
  const [mode, setMode] = useState<'live' | 'simulation'>('live')
  const [connectionOptionsOpen, setConnectionOptionsOpen] = useState(false)
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
  const connectionOptionsWrap = useRef<HTMLDivElement>(null)
  const connectionOptionsToggle = useRef<HTMLButtonElement>(null)
  const currentConnection = useRef(0)

  const connected = status === 'connected' || status === 'running'
  const busy = status === 'connecting' || status === 'disconnecting' || status === 'running'
  const txLocked = busy || autoTxRunning || txPending
  const canTransmit = status === 'connected' && !txLocked
  const serialDifferences = serialSettingDifferences(draft.serial, settings)
  const autoTxPayloadReady = autoTxSource === 'file' ? Boolean(autoTxFilePath) : Boolean(txInput)
  const settingsLocked = connected || busy
  const hasRxBytes = history.some(entry => entry.direction === 'rx' && entry.bytes.length > 0)
  const isContinuous = rxDisplay === 'continuous-text' || rxDisplay === 'continuous-hex'
  const rxText = useMemo(() => rxTextFragments(history, rxDisplay === 'continuous-text'), [history, rxDisplay])
  const rxHex = useMemo(() => rxDisplay === 'continuous-hex' ? continuousRxHex(history) : '', [history, rxDisplay])
  const nextThemePreference = nextTheme(theme)
  const nextThemeLabel = themeLabel(nextThemePreference)

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

  useEffect(() => {
    const view = terminalEnd.current?.parentElement
    if (view) view.scrollTop = view.scrollHeight
  }, [history, tab, rxDisplay, showTx])

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
    if (mode === 'live' && !settings.port) { setNotice({ message: 'Select a port before connecting.', kind: 'error' }); return }
    if (!Number.isInteger(settings.baud_rate) || settings.baud_rate <= 0) { setNotice({ message: 'Baud rate must be positive.', kind: 'error' }); return }
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
    setHistory([])
    try {
      await invoke('connect_serial', { mode, settings, events: channel })
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
      setNotice({ message: 'Interval must be integer milliseconds greater than zero.', kind: 'error' })
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
      const path = await open({ multiple: false, title: 'Auto TX File (Raw)' })
      if (path && typeof path === 'string') setAutoTxFilePath(path)
    } catch (error) { setNotice({ message: errorMessage(error), kind: 'error' }) }
  }

  async function sendFile() {
    if (!canTransmit) return
    const connectionId = currentConnection.current
    setTxPending(true)
    try {
      const path = await open({ multiple: false, title: 'Send File (Raw)' })
      if (!path || typeof path !== 'string' || connectionId !== currentConnection.current) return
      await invoke('send_serial_file', { path })
      setNotice({ message: `Sent raw bytes from ${path}`, kind: 'success' })
    } catch (error) { setNotice({ message: errorMessage(error), kind: 'error' }) }
    finally { setTxPending(false) }
  }

  async function exportTerminalLog() {
    const content = history.map(entry => `${entry.direction.toUpperCase()} ${hex(entry.bytes)}`).join('\n') + '\n'
    const timestamp = exportTimestamp()
    try {
      const path = await save({
        defaultPath: `terminal-log_${timestamp}.log`,
        filters: [{ name: 'Terminal Log (Hex)', extensions: ['log'] }],
      })
      if (!path) return
      await invoke('save_text_file', { path, content })
      setNotice({ message: `Saved ${path}`, kind: 'success' })
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
        filters: [{ name: 'Raw Binary', extensions: ['bin'] }],
      })
      if (!path) return
      await invoke('save_binary_file', { path, bytes })
      setNotice({ message: `Saved ${path}`, kind: 'success' })
    } catch (error) { setNotice({ message: errorMessage(error), kind: 'error' }) }
  }

  async function saveResult() {
    if (!lastRun) return
    const content = JSON.stringify(lastRun, null, 2) + '\n'
    try {
      const path = await save({ defaultPath: 'sequence-result.json', filters: [{ name: 'Sequence Result JSON', extensions: ['json'] }] })
      if (!path) return
      await invoke('save_text_file', { path, content })
      setNotice({ message: `Saved ${path}`, kind: 'success' })
    } catch (error) { setNotice({ message: errorMessage(error), kind: 'error' }) }
  }

  function json(): string { return JSON.stringify(draft) }

  async function validate() {
    try {
      await invoke('validate_sequence', { json: json() })
      setNotice({ message: 'Sequence is valid.', kind: 'success' })
    } catch (error) { setNotice({ message: errorMessage(error), kind: 'error' }) }
  }

  async function loadSequence() {
    try {
      const path = await open({ multiple: false, filters: [{ name: 'Sequence JSON', extensions: ['json'] }] })
      if (!path || typeof path !== 'string') return
      const normalized = await invoke<string>('load_sequence', { path })
      setDraft(JSON.parse(normalized) as Sequence)
      setNotice({ message: `Loaded ${path}`, kind: 'info' })
    } catch (error) { setNotice({ message: errorMessage(error), kind: 'error' }) }
  }

  async function saveSequence() {
    try {
      await invoke('validate_sequence', { json: json() })
      const path = await save({ defaultPath: 'sequence.json', filters: [{ name: 'Sequence JSON', extensions: ['json'] }] })
      if (!path) return
      await invoke('save_sequence', { path, json: json() })
      setNotice({ message: `Saved ${path}`, kind: 'info' })
    } catch (error) { setNotice({ message: errorMessage(error), kind: 'error' }) }
  }

  async function runSequence() {
    if (!canTransmit || serialDifferences.length !== 0) return
    setStatus('running')
    setNotice({ message: 'Running sequence…', kind: 'info' })
    setLastRun(null)
    try {
      const result = await invoke<RunResult>('run_sequence', { json: json() })
      setLastRun(result)
      setNotice(result.status === 'success'
        ? { message: 'Sequence completed.', kind: 'success' }
        : { message: result.error ?? 'Sequence failed.', kind: 'error' })
    } catch (error) { setNotice({ message: errorMessage(error), kind: 'error' }) }
    setStatus(previous => previous === 'error' || previous === 'disconnected' ? previous : 'connected')
  }

  return <main>
    <header className="topbar"><div><h1>Serial Tool</h1>{applicationVersion && (
      <span className="subtitle">v{applicationVersion}</span>
    )}</div>
      <div className="appearance-control"><span>Appearance</span>
        <button type="button" aria-label={`Switch theme to ${nextThemeLabel}`} title={`Switch theme to ${nextThemeLabel}`}
          onClick={() => setTheme(nextThemePreference)}>◐ {themeLabel(theme)}</button>
      </div>
    </header>

    <section className="connection panel">
      <div className="section-heading"><h2>Connection Setup</h2><div className="connection-header-actions">
        <span className={`status ${status}`}>{mode === 'live' ? 'Live' : 'Simulation'} · {statusLabels[status]}</span>
        <div className="connection-options-wrap" ref={connectionOptionsWrap}>
          <button type="button" ref={connectionOptionsToggle} className="connection-options-toggle" title="Connection options" aria-label="Connection options"
            aria-expanded={connectionOptionsOpen} aria-controls="connection-options" onClick={() => setConnectionOptionsOpen(previous => !previous)}>
            <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" strokeWidth="1.8" aria-hidden="true">
              <path d="m9 3-.7 2.7-2 .9-2.5-.7-2 3.4 1.9 2v2.4l-1.9 2 2 3.4 2.5-.7 2 .9L9 22h4l.7-2.7 2-.9 2.5.7 2-3.4-1.9-2v-2.4l1.9-2-2-3.4-2.5.7-2-.9L13 3Z" />
              <circle cx="11" cy="12.5" r="3" />
            </svg>
          </button>
          <div id="connection-options" className="connection-options" hidden={!connectionOptionsOpen}>
            <h3>Connection options</h3>
            <fieldset disabled={settingsLocked} className="execution-mode">
              <legend>Execution mode</legend>
              <div className="execution-mode-choices">
                <label><input type="radio" name="execution-mode" value="live" checked={mode === 'live'} onChange={() => setMode('live')} />Live</label>
                <label><input type="radio" name="execution-mode" value="simulation" checked={mode === 'simulation'} onChange={() => setMode('simulation')} />Simulation</label>
              </div>
            </fieldset>
            <p className="connection-options-help">Simulation uses deterministic loopback and does not access a physical serial port.</p>
          </div>
        </div>
      </div></div>
      <div className="connection-resource-row">
        <label>Port<select disabled={settingsLocked || mode === 'simulation'} value={settings.port} onChange={event => updateSetting('port', event.target.value)}>
          <option value="">Select a port</option>{ports.map(port => <option key={port.port_name} value={port.port_name}>{port.port_name} · {portTypeLabel(port.kind)}{port.vid != null ? ` ${port.vid.toString(16).padStart(4, '0')}:${port.pid?.toString(16).padStart(4, '0')}` : ''}</option>)}
        </select></label>
        <button disabled={settingsLocked || mode === 'simulation'} onClick={() => void refreshPorts()}>Refresh ports</button>
      </div>
      {mode === 'live' && settings.port && <p className="port-detail">{(() => {
        const port = ports.find(value => value.port_name === settings.port)
        return port ? [port.manufacturer, port.product, port.serial_number].filter(Boolean).join(' · ') : ''
      })()}</p>}
      <div className="connection-serial-grid">
        <label>Baud rate<input disabled={settingsLocked} type="number" value={settings.baud_rate} onChange={event => updateSetting('baud_rate', event.target.value)} /></label>
        <label>Data bits<select disabled={settingsLocked} value={settings.data_bits} onChange={event => updateSetting('data_bits', event.target.value)}>{[5, 6, 7, 8].map(value => <option key={value}>{value}</option>)}</select></label>
        <label>Parity<select disabled={settingsLocked} value={settings.parity} onChange={event => updateSetting('parity', event.target.value)}>{['none', 'odd', 'even'].map(value => <option key={value} value={value}>{value[0].toUpperCase() + value.slice(1)}</option>)}</select></label>
        <label>Stop bits<select disabled={settingsLocked} value={settings.stop_bits} onChange={event => updateSetting('stop_bits', event.target.value)}>{[1, 2].map(value => <option key={value}>{value}</option>)}</select></label>
        <label>Flow control<select disabled={settingsLocked} value={settings.flow_control} onChange={event => updateSetting('flow_control', event.target.value)}>{['none', 'software', 'hardware'].map(value => <option key={value} value={value}>{value[0].toUpperCase() + value.slice(1)}</option>)}</select></label>
        <label>Timeout (ms)<input disabled={settingsLocked} type="number" value={settings.timeout_ms} onChange={event => updateSetting('timeout_ms', event.target.value)} /></label>
      </div>
      <div className="connection-actions">
        <button className="primary" disabled={settingsLocked || (mode === 'live' && !settings.port) || settings.baud_rate <= 0} onClick={() => void connect()}>Connect</button>
        <button disabled={status !== 'connected'} onClick={() => void disconnect()}>Disconnect</button>
      </div>
    </section>

    {notice && <div className={`notice notice-${notice.kind}`} role="status">{notice.message}</div>}

    <nav className="tabs" aria-label="Main views"><button className={tab === 'terminal' ? 'active' : ''} onClick={() => setTab('terminal')}>Terminal</button>
      <button className={tab === 'sequence' ? 'active' : ''} onClick={() => setTab('sequence')}>Sequence</button></nav>

    {tab === 'terminal' ? <section className="terminal panel">
      <div className="section-heading"><h2>Terminal</h2><div className="toolbar"><label>Display Mode <select aria-label="Display Mode" value={rxDisplay} onChange={event => setRxDisplay(event.target.value as TerminalDisplay)}>
        <optgroup label="TX/RX Events">
          <option value="hex">Events · Hex</option>
          <option value="text">Events · Text</option>
          <option value="both">Events · Both</option>
        </optgroup>
        <optgroup label="Continuous RX">
          <option value="continuous-text">Continuous RX · Text</option>
          <option value="continuous-hex">Continuous RX · Hex</option>
        </optgroup>
      </select></label>
        <label className="show-tx"><input type="checkbox" checked={showTx} disabled={isContinuous} onChange={event => setShowTx(event.target.checked)} />Show TX</label>
        <button onClick={() => { setHistory([]); setExportOpen(false) }}>Clear View</button>
        <div className="export-menu-wrap">
          <button type="button" disabled={!history.length} aria-expanded={exportOpen} aria-controls="terminal-export-menu"
            onClick={() => setExportOpen(previous => !previous)}>Export ▼</button>
          <div id="terminal-export-menu" className="export-menu" hidden={!exportOpen}>
            <button type="button" onClick={() => { setExportOpen(false); void exportTerminalLog() }}>Terminal Log (Hex)...</button>
            <button type="button" disabled={!hasRxBytes} onClick={() => { setExportOpen(false); void exportRaw('rx') }}>RX Raw (.bin)...</button>
            <button type="button" onClick={() => { setExportOpen(false); void exportRaw('tx-rx') }}>TX + RX Raw (.bin)...</button>
          </div>
        </div></div></div>
      <div className="terminal-history" aria-live="polite">{history.length === 0 && <p className="muted">Incoming bytes appear here automatically after connection.</p>}
        {isContinuous ? <code className={`terminal-continuous ${rxDisplay}`}>{rxDisplay === 'continuous-text' ? rxText.join('') : rxHex}</code> : history.map((entry, index) => (showTx || entry.direction === 'rx') && <div className={`terminal-entry ${entry.direction}`} key={index}>
          <span className="direction">{entry.direction.toUpperCase()}</span><code>{entry.direction === 'tx' ? hex(entry.bytes) :
            rxDisplay === 'hex' ? hex(entry.bytes) :
              rxDisplay === 'text' ? rxText[index] : `${hex(entry.bytes)}  |  ${rxText[index]}`}</code></div>)}
        <div ref={terminalEnd} /></div>
      <div className="send-box"><div className="section-heading"><h3>Send</h3><div className="segmented"><button disabled={txLocked} className={txFormat === 'text' ? 'active' : ''} onClick={() => setTxFormat('text')}>Text</button><button disabled={txLocked} className={txFormat === 'hex' ? 'active' : ''} onClick={() => setTxFormat('hex')}>Hex</button></div></div>
        <textarea aria-label="Send data" value={txInput} onChange={event => setTxInput(event.target.value)} placeholder={txFormat === 'hex' ? '4F 4B 0D 0A' : 'Exact UTF-8 text; no line ending added'} disabled={!connected || txLocked} />
        <div className="toolbar auto-tx-source"><label>Auto TX Source <select aria-label="Auto TX source" disabled={txLocked} value={autoTxSource} onChange={event => setAutoTxSource(event.target.value as 'input' | 'file')}>
          <option value="input">Input</option><option value="file">File (Raw)</option></select></label>
          {autoTxSource === 'file' && <><button disabled={txLocked} onClick={() => void chooseAutoTxFile()}>Choose File...</button>
            <span className="muted auto-tx-file">{autoTxFilePath ?? 'No file selected'}</span></>}
        </div>
        <div className="send-actions"><span className="muted">{txFormat === 'text' ? 'Text sends exact UTF-8 bytes.' : 'Hex accepts digits and ASCII whitespace.'} Send File sends raw bytes unchanged.</span>
          <div className="toolbar"><label>Interval <input aria-label="Auto TX interval (ms)" type="number" min="1" step="1" disabled={!connected || txLocked} value={intervalMs} onChange={event => setIntervalMs(event.target.value)} /> ms</label>
            <button className="primary" disabled={!canTransmit || !txInput} onClick={() => void send()}>Send</button>
            <button disabled={!canTransmit} onClick={() => void sendFile()}>Send File (Raw)...</button>
            <button disabled={status !== 'connected' || txPending || (!autoTxRunning && (busy || !autoTxPayloadReady))} onClick={() => void toggleAutoTx()}>{autoTxRunning ? 'Stop Auto TX' : 'Start Auto TX'}</button></div></div>
      </div>
    </section> : <><SequenceEditor draft={draft} onChange={setDraft} connection={settings} disabled={status === 'running'}
      isConnected={connected} serialDifferences={serialDifferences}
      onNew={() => setDraft(freshSequence(settings))} onLoad={() => void loadSequence()} onSave={() => void saveSequence()}
      onValidate={() => void validate()} onRun={() => void runSequence()} canRun={canTransmit && serialDifferences.length === 0} />
      <section className="panel results"><div className="section-heading"><h2>Last Sequence Run</h2><div className="toolbar"><button disabled={!lastRun} onClick={() => void saveResult()}>Save Result...</button><label>RX Display <select value={resultDisplay} onChange={event => setResultDisplay(event.target.value as Display)}>
        <option value="hex">Hex</option><option value="text">Text</option><option value="both">Both</option></select></label></div></div>
        {!lastRun ? <p className="muted">No sequence run in this session.</p> : <>
          <p className={`result-status ${lastRun.status}`}>Status: {lastRun.status}{lastRun.failing_step_id ? ` · ${lastRun.failing_step_id}` : ''}</p>
          {lastRun.error && <p className="error-text">{lastRun.error}</p>}
          {lastRun.partial_bytes && <p>Partial bytes: <code>{display(lastRun.partial_bytes, resultDisplay)}</code></p>}
          <h3>Step Results</h3><div className="result-list">{lastRun.step_results.map((result, index) => <div key={index} className="result-row"><strong>{result.step_id}</strong><span>{result.kind}</span><code>{result.bytes ? display(result.bytes, resultDisplay) : result.bytes_written != null ? `${result.bytes_written} bytes written` : result.requested_duration_ms != null ? `${result.requested_duration_ms} ms` : result.completed_iterations != null ? `${result.completed_iterations} iterations` : ''}</code></div>)}</div>
          <h3>Transcript</h3><div className="result-list">{lastRun.transcript.map((entry, index) => <div key={index} className="result-row"><strong>{entry.direction.toUpperCase()}</strong><span>{entry.step_id}</span><code>{entry.direction === 'rx' ? display(entry.bytes, resultDisplay) : hex(entry.bytes)}</code></div>)}</div>
        </>}
      </section></>}
  </main>
}
