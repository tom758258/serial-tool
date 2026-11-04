export type LineSettings = {
  baud_rate: number
  data_bits: number
  parity: string
  stop_bits: number
  flow_control: string
  timeout_ms: number
}

export type ConnectionSettings = LineSettings & { port: string }
export type Transport = 'serial' | 'tcp'
export type TcpConnectionSettings = {
  host: string
  port: string
  connect_timeout_ms: string
  io_timeout_ms: string
}

export const defaultTcpConnection: TcpConnectionSettings = {
  host: '', port: '', connect_timeout_ms: '3000', io_timeout_ms: '1000',
}

function positiveInteger(value: string, label: string): number {
  if (!/^[0-9]+$/.test(value.trim())) throw new Error(`${label} must be a positive integer.`)
  const parsed = Number(value)
  if (!Number.isSafeInteger(parsed) || parsed <= 0) throw new Error(`${label} must be a positive integer.`)
  return parsed
}

export function tcpConnectSettings(settings: TcpConnectionSettings, mode: 'live' | 'simulation') {
  const connect_timeout_ms = positiveInteger(settings.connect_timeout_ms, 'Connect timeout (ms)')
  const io_timeout_ms = positiveInteger(settings.io_timeout_ms, 'I/O timeout (ms)')
  if (mode === 'simulation') {
    // Core's TCP Simulation does not resolve a host or open a socket.
    return { host: '', port: 0, connect_timeout_ms, io_timeout_ms }
  }
  const host = settings.host.trim()
  if (!host || host.includes('://') || host.includes('/')) {
    throw new Error('TCP Host must be a nonempty hostname or IP address, not a URL.')
  }
  const port = positiveInteger(settings.port, 'TCP Port')
  if (port > 65535) throw new Error('TCP Port must be 1–65535.')
  return { host, port, connect_timeout_ms, io_timeout_ms }
}

export function serialSettingDifferences(required: LineSettings, current: LineSettings): (keyof LineSettings)[] {
  const fields: (keyof LineSettings)[] = ['baud_rate', 'data_bits', 'parity', 'stop_bits', 'flow_control', 'timeout_ms']
  return fields.filter(field => required[field] !== current[field])
}

export type Step =
  | { id: string; type: 'send_text'; text: string }
  | { id: string; type: 'send_bytes'; hex: string }
  | { id: string; type: 'wait'; duration_ms: number }
  | { id: string; type: 'read'; max_bytes: number }
  | { id: string; type: 'read_until'; delimiter_hex: string; max_bytes: number }
  | { id: string; type: 'repeat'; count: number; steps: Step[] }

export type SequenceTransport =
  | { type: 'serial'; settings: LineSettings }
  | { type: 'tcp'; settings: { io_timeout_ms: number } }

export type Sequence = {
  sequence_version: number
  transport: SequenceTransport
  steps: Step[]
}

export function sequenceMatches(sequence: Sequence, transport: Transport,
  serial: LineSettings, tcp: TcpConnectionSettings): boolean {
  if (sequence.transport.type !== transport) return false
  if (sequence.transport.type === 'serial') {
    return serialSettingDifferences(sequence.transport.settings, serial).length === 0
  }
  return sequence.transport.settings.io_timeout_ms === Number(tcp.io_timeout_ms)
}

export type Port = {
  port_name: string
  kind: string
  vid: number | null
  pid: number | null
  serial_number: string | null
  manufacturer: string | null
  product: string | null
}

export type DataEvent = {
  kind: 'data'
  direction: 'tx' | 'rx'
  bytes: number[]
}

export type SessionEvent = DataEvent | { kind: 'connected' | 'disconnected' } | {
  kind: 'connection_error'
  message: string
}

export type StepResult = {
  step_id: string
  kind: string
  bytes_written: number | null
  requested_duration_ms: number | null
  bytes: number[] | null
  completed_iterations: number | null
}

export type RunResult = {
  status: 'success' | 'failed'
  failing_step_id: string | null
  error: string | null
  partial_bytes: number[] | null
  step_results: StepResult[]
  transcript: { step_id: string; direction: 'tx' | 'rx'; bytes: number[] }[]
}

export const defaultLine: LineSettings = {
  baud_rate: 115200,
  data_bits: 8,
  parity: 'none',
  stop_bits: 1,
  flow_control: 'none',
  timeout_ms: 1000,
}

export function freshSequence(line: LineSettings, transport: Transport = 'serial', ioTimeoutMs = 1000): Sequence {
  if (transport === 'tcp') {
    return {
      sequence_version: 1,
      transport: { type: 'tcp', settings: { io_timeout_ms: ioTimeoutMs } },
      steps: [],
    }
  }
  return {
    sequence_version: 1,
    transport: { type: 'serial', settings: {
      baud_rate: line.baud_rate,
      data_bits: line.data_bits,
      parity: line.parity,
      stop_bits: line.stop_bits,
      flow_control: line.flow_control,
      timeout_ms: line.timeout_ms,
    } },
    steps: [],
  }
}

export function hex(bytes: number[]): string {
  return bytes.map(byte => byte.toString(16).toUpperCase().padStart(2, '0')).join(' ')
}

function escapeText(decoded: string, stream = false): string {
  return [...decoded].map(char => {
    if (stream && char === '\r') return ''
    if (stream && char === '\n') return '\n'
    if (char === '\r') return '\\r'
    if (char === '\n') return '\\n'
    if (char === '\t') return '\\t'
    const code = char.codePointAt(0)!
    if (code < 0x20 || code === 0x7f || (code >= 0x80 && code <= 0x9f)) {
      return `\\u{${code.toString(16).toUpperCase()}}`
    }
    return char
  }).join('')
}

export function text(bytes: number[]): string {
  return escapeText(new TextDecoder('utf-8', { fatal: false }).decode(new Uint8Array(bytes)))
}

export function rxTextFragments(entries: { direction: 'tx' | 'rx'; bytes: number[] }[], stream = false): string[] {
  const decoder = new TextDecoder('utf-8', { fatal: false })
  return entries.map(entry => entry.direction === 'rx'
    ? escapeText(decoder.decode(new Uint8Array(entry.bytes), { stream: true }), stream)
    : '')
}

export function continuousRxHex(entries: { direction: 'tx' | 'rx'; bytes: number[] }[]): string {
  return entries.filter(entry => entry.direction === 'rx' && entry.bytes.length > 0)
    .map(entry => hex(entry.bytes))
    .join(' ')
}

export type Display = 'hex' | 'text' | 'both'
export type TerminalDisplay = Display | 'continuous-text' | 'continuous-hex'

export function display(bytes: number[], mode: Display): string {
  if (mode === 'hex') return hex(bytes)
  if (mode === 'text') return text(bytes)
  return `${hex(bytes)}  |  ${text(bytes)}`
}
