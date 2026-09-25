import { useEffect, useMemo, useRef, useState } from 'react'
import { invoke, Channel } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { open, save } from '@tauri-apps/plugin-dialog'
import SequenceEditor from './SequenceEditor'
import { defaultLine, display, freshSequence, hex, rxTextFragments } from './model'
import type { ConnectionSettings, Display, Port, RunResult, Sequence, SessionEvent } from './model'

type Status = 'disconnected' | 'connecting' | 'connected' | 'running' | 'disconnecting' | 'error'
type Theme = 'system' | 'light' | 'dark'
type HistoryEntry = { direction: 'tx' | 'rx'; bytes: number[] }
type Notice = { message: string; kind: 'info' | 'success' | 'error' }
const HISTORY_LIMIT = 5000

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

function initialTheme(): Theme {
  const saved = localStorage.getItem('serial-tool.theme')
  return saved === 'light' || saved === 'dark' ? saved : 'system'
}

export default function App() {
  const [theme, setTheme] = useState<Theme>(initialTheme)
  const [mode, setMode] = useState<'live' | 'simulation'>('live')
  const [settings, setSettings] = useState<ConnectionSettings>({ port: '', ...defaultLine })
  const [ports, setPorts] = useState<Port[]>([])
  const [status, setStatus] = useState<Status>('disconnected')
  const [notice, setNotice] = useState<Notice | null>(null)
  const [tab, setTab] = useState<'terminal' | 'sequence'>('terminal')
  const [txFormat, setTxFormat] = useState<'text' | 'hex'>('text')
  const [txInput, setTxInput] = useState('')
  const [rxDisplay, setRxDisplay] = useState<Display>('hex')
  const [history, setHistory] = useState<HistoryEntry[]>([])
  const [draft, setDraft] = useState<Sequence>(() => freshSequence(defaultLine))
  const [lastRun, setLastRun] = useState<RunResult | null>(null)
  const [resultDisplay, setResultDisplay] = useState<Display>('hex')
  const terminalEnd = useRef<HTMLDivElement>(null)
  const currentConnection = useRef(0)

  const connected = status === 'connected' || status === 'running'
  const busy = status === 'connecting' || status === 'disconnecting' || status === 'running'
  const settingsLocked = connected || busy
  const rxText = useMemo(() => rxTextFragments(history), [history])

  useEffect(() => {
    localStorage.setItem('serial-tool.theme', theme)
    const media = window.matchMedia('(prefers-color-scheme: dark)')
    const apply = () => { document.documentElement.dataset.theme = theme === 'system' ? (media.matches ? 'dark' : 'light') : theme }
    apply()
    media.addEventListener('change', apply)
    getCurrentWindow().setTheme(theme === 'system' ? null : theme).catch(error => console.warn('Native theme:', error))
    return () => media.removeEventListener('change', apply)
  }, [theme])

  useEffect(() => { terminalEnd.current?.scrollIntoView({ block: 'end' }) }, [history.length, tab])

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
    setStatus('connecting')
    setNotice(null)
    const connectionId = ++currentConnection.current
    const channel = new Channel<SessionEvent>()
    channel.onmessage = event => {
      if (connectionId !== currentConnection.current) return
      if (event.kind === 'data') {
        setHistory(previous => [...previous, { direction: event.direction, bytes: event.bytes }].slice(-HISTORY_LIMIT))
      } else if (event.kind === 'connection_error') {
        setNotice({ message: event.message, kind: 'error' })
        setStatus('error')
      } else if (event.kind === 'disconnected') {
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
      setStatus('disconnected')
      setNotice(null)
    } catch (error) {
      setStatus('error')
      setNotice({ message: errorMessage(error), kind: 'error' })
    }
  }

  async function send() {
    try {
      await invoke('send_serial', { input: txInput, format: txFormat })
      setNotice(null)
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
    <header className="topbar"><div><h1>Serial Tool</h1><span className="subtitle">Desktop console</span></div>
      <label className="theme-control">Theme <select value={theme} onChange={event => setTheme(event.target.value as Theme)}>
        <option value="system">System</option><option value="light">Light</option><option value="dark">Dark</option>
      </select></label>
    </header>

    <section className="connection panel">
      <div className="section-heading"><h2>Connection</h2><span className={`status ${status}`}>{status === 'running' ? 'Running…' : status}</span></div>
      <div className="field-grid connection-fields">
        <label>Mode<select disabled={settingsLocked} value={mode} onChange={event => setMode(event.target.value as 'live' | 'simulation')}>
          <option value="live">Live</option><option value="simulation">Simulation</option>
        </select></label>
        <label>Port<select disabled={settingsLocked || mode === 'simulation'} value={settings.port} onChange={event => updateSetting('port', event.target.value)}>
          <option value="">Select a port</option>{ports.map(port => <option key={port.port_name} value={port.port_name}>{port.port_name} · {port.kind}{port.vid != null ? ` ${port.vid.toString(16).padStart(4, '0')}:${port.pid?.toString(16).padStart(4, '0')}` : ''}</option>)}
        </select></label>
        <button disabled={settingsLocked} onClick={() => void refreshPorts()}>Refresh Ports</button>
        <label>Baud<input disabled={settingsLocked} type="number" value={settings.baud_rate} onChange={event => updateSetting('baud_rate', event.target.value)} /></label>
        <label>Data bits<select disabled={settingsLocked} value={settings.data_bits} onChange={event => updateSetting('data_bits', event.target.value)}>{[5, 6, 7, 8].map(value => <option key={value}>{value}</option>)}</select></label>
        <label>Parity<select disabled={settingsLocked} value={settings.parity} onChange={event => updateSetting('parity', event.target.value)}>{['none', 'odd', 'even'].map(value => <option key={value}>{value}</option>)}</select></label>
        <label>Stop bits<select disabled={settingsLocked} value={settings.stop_bits} onChange={event => updateSetting('stop_bits', event.target.value)}>{[1, 2].map(value => <option key={value}>{value}</option>)}</select></label>
        <label>Flow control<select disabled={settingsLocked} value={settings.flow_control} onChange={event => updateSetting('flow_control', event.target.value)}>{['none', 'software', 'hardware'].map(value => <option key={value}>{value}</option>)}</select></label>
        <label>Timeout ms<input disabled={settingsLocked} type="number" value={settings.timeout_ms} onChange={event => updateSetting('timeout_ms', event.target.value)} /></label>
      </div>
      {mode === 'live' && settings.port && <p className="port-detail">{(() => {
        const port = ports.find(value => value.port_name === settings.port)
        return port ? [port.manufacturer, port.product, port.serial_number].filter(Boolean).join(' · ') : ''
      })()}</p>}
      <div className="connection-actions">
        <button className="primary" disabled={settingsLocked || (mode === 'live' && !settings.port) || settings.baud_rate <= 0} onClick={() => void connect()}>Connect</button>
        <button disabled={status !== 'connected'} onClick={() => void disconnect()}>Disconnect</button>
      </div>
    </section>

    {notice && <div className={`notice notice-${notice.kind}`} role="status">{notice.message}</div>}

    <nav className="tabs" aria-label="Main views"><button className={tab === 'terminal' ? 'active' : ''} onClick={() => setTab('terminal')}>Terminal</button>
      <button className={tab === 'sequence' ? 'active' : ''} onClick={() => setTab('sequence')}>Sequence</button></nav>

    {tab === 'terminal' ? <section className="terminal panel">
      <div className="section-heading"><h2>Terminal</h2><div className="toolbar"><label>RX Display <select value={rxDisplay} onChange={event => setRxDisplay(event.target.value as Display)}>
        <option value="hex">Hex</option><option value="text">Text</option><option value="both">Both</option></select></label>
        <button onClick={() => setHistory([])}>Clear View</button></div></div>
      <div className="terminal-history" aria-live="polite">{history.length === 0 && <p className="muted">Incoming bytes appear here automatically after connection.</p>}
        {history.map((entry, index) => <div className={`terminal-entry ${entry.direction}`} key={index}>
          <span className="direction">{entry.direction.toUpperCase()}</span><code>{entry.direction === 'tx' ? hex(entry.bytes) :
            rxDisplay === 'hex' ? hex(entry.bytes) :
              rxDisplay === 'text' ? rxText[index] : `${hex(entry.bytes)}  |  ${rxText[index]}`}</code></div>)}
        <div ref={terminalEnd} /></div>
      <div className="send-box"><div className="section-heading"><h3>Send</h3><div className="segmented"><button className={txFormat === 'text' ? 'active' : ''} onClick={() => setTxFormat('text')}>Text</button><button className={txFormat === 'hex' ? 'active' : ''} onClick={() => setTxFormat('hex')}>Hex</button></div></div>
        <textarea aria-label="Send data" value={txInput} onChange={event => setTxInput(event.target.value)} placeholder={txFormat === 'hex' ? '4F 4B 0D 0A' : 'Exact UTF-8 text; no line ending added'} disabled={!connected || busy} />
        <div className="send-actions"><span className="muted">{txFormat === 'text' ? 'Text sends exact UTF-8 bytes.' : 'Hex accepts digits and ASCII whitespace.'}</span><button className="primary" disabled={status !== 'connected' || !txInput} onClick={() => void send()}>Send</button></div>
      </div>
    </section> : <><SequenceEditor draft={draft} onChange={setDraft} connection={settings} disabled={status === 'running'}
      onNew={() => setDraft(freshSequence(settings))} onLoad={() => void loadSequence()} onSave={() => void saveSequence()}
      onValidate={() => void validate()} onRun={() => void runSequence()} canRun={status === 'connected'} />
      <section className="panel results"><div className="section-heading"><h2>Last Sequence Run</h2><label>RX Display <select value={resultDisplay} onChange={event => setResultDisplay(event.target.value as Display)}>
        <option value="hex">Hex</option><option value="text">Text</option><option value="both">Both</option></select></label></div>
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
