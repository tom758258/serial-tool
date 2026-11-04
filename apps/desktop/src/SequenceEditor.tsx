import { useState } from 'react'
import { useI18n } from './i18n/I18nProvider'
import type { ConnectionSettings, LineSettings, Sequence, Step, TcpConnectionSettings, Transport } from './model'

type Props = {
  draft: Sequence
  onChange: (next: Sequence) => void
  connection: ConnectionSettings
  tcpConnection: TcpConnectionSettings
  connectionTransport: Transport
  disabled: boolean
  isConnected: boolean
  serialDifferences: (keyof LineSettings)[]
  onNew: () => void
  onLoad: () => void
  onSave: () => void
  onValidate: () => void
  onRun: () => void
  canRun: boolean
}

const kinds: Step['type'][] = ['send_text', 'send_bytes', 'wait', 'read', 'read_until', 'repeat']
function atPath(steps: Step[], path: number[]): Step {
  let current = steps[path[0]]
  for (const index of path.slice(1)) {
    if (current.type !== 'repeat') throw new Error('Invalid step path')
    current = current.steps[index]
  }
  return current
}

function siblings(steps: Step[], path: number[]): Step[] {
  if (path.length === 1) return steps
  const parent = atPath(steps, path.slice(0, -1))
  if (parent.type !== 'repeat') throw new Error('Invalid repeat path')
  return parent.steps
}

function collectIds(steps: Step[]): Set<string> {
  const ids = new Set<string>()
  const visit = (items: Step[]) => items.forEach(step => {
    ids.add(step.id)
    if (step.type === 'repeat') visit(step.steps)
  })
  visit(steps)
  return ids
}

function newStep(kind: Step['type'], steps: Step[]): Step {
  const ids = collectIds(steps)
  const prefix = kind.replaceAll('_', '-')
  let number = 1
  while (ids.has(`${prefix}-${number}`)) number++
  const id = `${prefix}-${number}`
  switch (kind) {
    case 'send_text': return { id, type: kind, text: '' }
    case 'send_bytes': return { id, type: kind, hex: '' }
    case 'wait': return { id, type: kind, duration_ms: 0 }
    case 'read': return { id, type: kind, max_bytes: 64 }
    case 'read_until': return { id, type: kind, delimiter_hex: '0D0A', max_bytes: 64 }
    case 'repeat': return { id, type: kind, count: 1, steps: [] }
  }
}

function numeric(value: string): number {
  return value === '' ? 0 : Number(value)
}

export default function SequenceEditor(props: Props) {
  const { t } = useI18n()
  const { draft, onChange, connection, tcpConnection, connectionTransport, disabled, isConnected, serialDifferences } = props
  const [selected, setSelected] = useState<number[] | null>(null)
  const [addKind, setAddKind] = useState<Step['type']>('send_text')
  const step = selected ? (() => { try { return atPath(draft.steps, selected) } catch { return null } })() : null

  function edit(change: (next: Sequence) => void) {
    const next = structuredClone(draft)
    change(next)
    onChange(next)
  }

  function add(child: boolean) {
    edit(next => {
      const target = child && selected ? atPath(next.steps, selected) : null
      const list = target?.type === 'repeat' ? target.steps : next.steps
      list.push(newStep(addKind, next.steps))
      setSelected(target?.type === 'repeat' && selected ? [...selected, list.length - 1] : [list.length - 1])
    })
  }

  function remove() {
    if (!selected) return
    edit(next => {
      const list = siblings(next.steps, selected)
      list.splice(selected.at(-1)!, 1)
    })
    setSelected(null)
  }

  function move(delta: number) {
    if (!selected) return
    const index = selected.at(-1)!
    const list = siblings(draft.steps, selected)
    const destination = index + delta
    if (destination < 0 || destination >= list.length) return
    edit(next => {
      const items = siblings(next.steps, selected)
      ;[items[index], items[destination]] = [items[destination], items[index]]
    })
    setSelected([...selected.slice(0, -1), destination])
  }

  function updateStep(patch: Partial<Step>) {
    if (!selected) return
    edit(next => Object.assign(atPath(next.steps, selected), patch))
  }

  function updateSerial(key: keyof LineSettings, value: string) {
    edit(next => {
      if (next.transport.type === 'serial') {
        (next.transport.settings as unknown as Record<string, string | number>)[key] =
          ['baud_rate', 'data_bits', 'stop_bits', 'timeout_ms'].includes(key) ? numeric(value) : value
      }
    })
  }

  function renderSteps(steps: Step[], prefix: number[] = []) {
    return steps.map((item, index) => {
      const path = [...prefix, index]
      const active = selected?.join('.') === path.join('.')
      return <div className="step-tree" key={path.join('.')}>
        <button type="button" disabled={disabled} className={`step-row ${active ? 'selected' : ''}`}
          onClick={() => setSelected(path)}>
          <span>{t('sequence.step.' + item.type)}</span><strong>{item.id || '(no ID)'}</strong>
        </button>
        {item.type === 'repeat' && <div className="nested-steps">{renderSteps(item.steps, path)}</div>}
      </div>
    })
  }

  const serial = draft.transport.type === 'serial' ? draft.transport.settings : null

  return <div className="sequence-editor">
    <div className="toolbar">
      <button disabled={disabled} onClick={() => { setSelected(null); props.onNew() }}>{t('sequence.new')}</button>
      <button disabled={disabled} onClick={() => { setSelected(null); props.onLoad() }}>{t('sequence.load')}</button>
      <button disabled={disabled} onClick={props.onSave}>{t('sequence.save')}</button>
      <button disabled={disabled} onClick={props.onValidate}>{t('sequence.validate')}</button>
      <button className="primary" disabled={!props.canRun || disabled} onClick={props.onRun}>{t('sequence.run')}</button>
    </div>
    <div className="sequence-serial panel">
      <div className="section-heading"><h3>{t(serial ? 'sequence.required_serial' : 'sequence.required_tcp')}</h3>
        <button disabled={disabled || draft.transport.type !== connectionTransport} onClick={() => edit(next => {
          if (next.transport.type === 'serial' && connectionTransport === 'serial') {
            next.transport.settings = {
              baud_rate: connection.baud_rate, data_bits: connection.data_bits,
              parity: connection.parity, stop_bits: connection.stop_bits,
              flow_control: connection.flow_control, timeout_ms: connection.timeout_ms,
            }
          } else if (next.transport.type === 'tcp' && connectionTransport === 'tcp') {
            next.transport.settings.io_timeout_ms = Number(tcpConnection.io_timeout_ms)
          }
        })}>{t('sequence.copy_settings')}</button></div>
      <p className="muted">{t('sequence.settings_help')}</p>
      {serial ? <>
        <div className="field-grid">
          <label>{t('connection.baud_rate')}<input disabled={disabled} type="number" value={serial.baud_rate} onChange={event => updateSerial('baud_rate', event.target.value)} /></label>
          <label>{t('connection.data_bits')}<select disabled={disabled} value={serial.data_bits} onChange={event => updateSerial('data_bits', event.target.value)}>{[5, 6, 7, 8].map(value => <option key={value}>{value}</option>)}</select></label>
          <label>{t('connection.parity')}<select disabled={disabled} value={serial.parity} onChange={event => updateSerial('parity', event.target.value)}>{['none', 'odd', 'even'].map(value => <option key={value} value={value}>{t('common.choice.' + value)}</option>)}</select></label>
          <label>{t('connection.stop_bits')}<select disabled={disabled} value={serial.stop_bits} onChange={event => updateSerial('stop_bits', event.target.value)}>{[1, 2].map(value => <option key={value}>{value}</option>)}</select></label>
          <label>{t('connection.flow_control')}<select disabled={disabled} value={serial.flow_control} onChange={event => updateSerial('flow_control', event.target.value)}>{['none', 'software', 'hardware'].map(value => <option key={value} value={value}>{t('common.choice.' + value)}</option>)}</select></label>
          <label>{t('connection.timeout')}<input disabled={disabled} type="number" value={serial.timeout_ms} onChange={event => updateSerial('timeout_ms', event.target.value)} /></label>
        </div>
        <div className="sequence-serial-status" role="status" aria-live="polite">
          {!isConnected ? <span className="muted">{t('sequence.not_connected')}</span>
            : connectionTransport !== 'serial'
              ? <span className="sequence-serial-mismatch">{t('sequence.mismatch_serial')}</span>
            : serialDifferences.length === 0 ? <span className="sequence-serial-match">{t('sequence.settings_match')}</span>
              : <div><span className="sequence-serial-mismatch">{t('sequence.settings_mismatch')}</span>
                <ul className="sequence-serial-differences">{serialDifferences.map(field =>
                  <li key={field}>{t('sequence.mismatch_entry', { field: t('connection.' + field), sequence: String(serial[field]), connection: String(connection[field]) })}</li>)}</ul>
              </div>}
        </div>
      </> : draft.transport.type === 'tcp' && <>
        <div className="field-grid">
          <label>{t('connection.io_timeout')}<input disabled={disabled} type="number" min="1"
            value={draft.transport.settings.io_timeout_ms}
            onChange={event => edit(next => {
              if (next.transport.type === 'tcp') next.transport.settings.io_timeout_ms = numeric(event.target.value)
            })} /></label>
        </div>
        <div className="sequence-serial-status" role="status" aria-live="polite">
          {!isConnected ? <span className="muted">{t('sequence.not_connected')}</span>
            : connectionTransport !== 'tcp'
              ? <span className="sequence-serial-mismatch">{t('sequence.mismatch_tcp')}</span>
              : draft.transport.settings.io_timeout_ms === Number(tcpConnection.io_timeout_ms)
                ? <span className="sequence-serial-match">{t('sequence.settings_match')}</span>
                : <span className="sequence-serial-mismatch">{t('sequence.io_mismatch', { sequence: draft.transport.settings.io_timeout_ms, connection: tcpConnection.io_timeout_ms })}</span>}
        </div>
      </>}
    </div>
    <div className="editor-columns">
      <section className="panel palette"><h3>{t('sequence.palette')}</h3>
        <label>{t('sequence.step_type')}<select disabled={disabled} value={addKind} onChange={event => setAddKind(event.target.value as Step['type'])}>
          {kinds.map(kind => <option value={kind} key={kind}>{t('sequence.step.' + kind)}</option>)}
        </select></label>
        <button disabled={disabled} onClick={() => add(false)}>{t('sequence.add_step')}</button>
        <button disabled={disabled || step?.type !== 'repeat'} onClick={() => add(true)}>{t('sequence.add_child')}</button>
      </section>
      <section className="panel steps-panel"><h3>{t('sequence.steps')}</h3>
        <div className="step-list">{draft.steps.length ? renderSteps(draft.steps) : <p className="muted">{t('sequence.add_hint')}</p>}</div>
        <div className="toolbar"><button disabled={disabled || !step} onClick={remove}>{t('sequence.delete')}</button>
          <button disabled={disabled || !step} onClick={() => move(-1)}>{t('sequence.move_up')}</button>
          <button disabled={disabled || !step} onClick={() => move(1)}>{t('sequence.move_down')}</button></div>
      </section>
      <section className="panel properties"><h3>{t('sequence.properties')}</h3>
        {!step ? <p className="muted">{t('sequence.select_step')}</p> : <div className="properties-fields">
          <p className="step-kind">{t('sequence.step.' + step.type)}</p>
          <p className="muted step-description">{t('sequence.desc.' + step.type)}</p>
          <label>ID<input disabled={disabled} value={step.id} onChange={event => updateStep({ id: event.target.value })} /></label>
          {step.type === 'send_text' && <label>{t('common.text')}<textarea disabled={disabled} value={step.text} onChange={event => updateStep({ text: event.target.value })} /></label>}
          {step.type === 'send_bytes' && <label>{t('common.hex')}<textarea disabled={disabled} value={step.hex} onChange={event => updateStep({ hex: event.target.value })} /></label>}
          {step.type === 'wait' && <label>{t('sequence.duration')}<input disabled={disabled} type="number" value={step.duration_ms} onChange={event => updateStep({ duration_ms: numeric(event.target.value) })} /></label>}
          {(step.type === 'read' || step.type === 'read_until') && <label>{t('sequence.max_bytes')}<input disabled={disabled} type="number" value={step.max_bytes} onChange={event => updateStep({ max_bytes: numeric(event.target.value) })} /></label>}
          {step.type === 'read_until' && <label>{t('sequence.delimiter_hex')}<input disabled={disabled} value={step.delimiter_hex} onChange={event => updateStep({ delimiter_hex: event.target.value })} /></label>}
          {step.type === 'repeat' && <label>{t('sequence.count')}<input disabled={disabled} type="number" value={step.count} onChange={event => updateStep({ count: numeric(event.target.value) })} /></label>}
        </div>}
      </section>
    </div>
  </div>
}
