export type LineSettings = {
  baud_rate: number
  data_bits: number
  parity: string
  stop_bits: number
  flow_control: string
  timeout_ms: number
}

export type ConnectionSettings = LineSettings & { port: string }

export type Step =
  | { id: string; type: 'send_text'; text: string }
  | { id: string; type: 'send_bytes'; hex: string }
  | { id: string; type: 'wait'; duration_ms: number }
  | { id: string; type: 'read'; max_bytes: number }
  | { id: string; type: 'read_until'; delimiter_hex: string; max_bytes: number }
  | { id: string; type: 'repeat'; count: number; steps: Step[] }

export type Sequence = {
  sequence_version: number
  serial: LineSettings
  steps: Step[]
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

export function freshSequence(line: LineSettings): Sequence {
  return {
    sequence_version: 1,
    serial: {
      baud_rate: line.baud_rate,
      data_bits: line.data_bits,
      parity: line.parity,
      stop_bits: line.stop_bits,
      flow_control: line.flow_control,
      timeout_ms: line.timeout_ms,
    },
    steps: [],
  }
}

export function hex(bytes: number[]): string {
  return bytes.map(byte => byte.toString(16).toUpperCase().padStart(2, '0')).join(' ')
}

export function text(bytes: number[]): string {
  const decoded = new TextDecoder('utf-8', { fatal: false }).decode(new Uint8Array(bytes))
  return [...decoded].map(char => {
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

export type Display = 'hex' | 'text' | 'both'

export function display(bytes: number[], mode: Display): string {
  if (mode === 'hex') return hex(bytes)
  if (mode === 'text') return text(bytes)
  return `${hex(bytes)}  |  ${text(bytes)}`
}
