import { useState } from 'react'
import type { ConnectionSettings, LineSettings, Sequence, Step } from './model'

type Props = {
  draft: Sequence
  onChange: (next: Sequence) => void
  connection: ConnectionSettings
  disabled: boolean
  onNew: () => void
  onLoad: () => void
  onSave: () => void
  onValidate: () => void
  onRun: () => void
  canRun: boolean
}

const kinds: Step['type'][] = ['send_text', 'send_bytes', 'wait', 'read', 'read_until', 'repeat']
const label = (kind: Step['type']) => kind.replace('_', ' ').replace(/\b\w/g, letter => letter.toUpperCase())

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
  const { draft, onChange, connection, disabled } = props
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
    edit(next => { (next.serial as unknown as Record<string, string | number>)[key] =
      ['baud_rate', 'data_bits', 'stop_bits', 'timeout_ms'].includes(key) ? numeric(value) : value })
  }

  function renderSteps(steps: Step[], prefix: number[] = []) {
    return steps.map((item, index) => {
      const path = [...prefix, index]
      const active = selected?.join('.') === path.join('.')
      return <div className="step-tree" key={path.join('.')}>
        <button type="button" disabled={disabled} className={`step-row ${active ? 'selected' : ''}`}
          onClick={() => setSelected(path)}>
          <span>{label(item.type)}</span><strong>{item.id || '(no ID)'}</strong>
        </button>
        {item.type === 'repeat' && <div className="nested-steps">{renderSteps(item.steps, path)}</div>}
      </div>
    })
  }

  return <div className="sequence-editor">
    <div className="toolbar">
      <button disabled={disabled} onClick={() => { setSelected(null); props.onNew() }}>New</button>
      <button disabled={disabled} onClick={() => { setSelected(null); props.onLoad() }}>Load</button>
      <button disabled={disabled} onClick={props.onSave}>Save</button>
      <button disabled={disabled} onClick={props.onValidate}>Validate</button>
      <button className="primary" disabled={!props.canRun || disabled} onClick={props.onRun}>Run Sequence</button>
    </div>
    <div className="sequence-serial panel">
      <div className="section-heading"><h3>Sequence Serial Settings</h3>
        <button disabled={disabled} onClick={() => edit(next => { next.serial = {
          baud_rate: connection.baud_rate, data_bits: connection.data_bits,
          parity: connection.parity, stop_bits: connection.stop_bits,
          flow_control: connection.flow_control, timeout_ms: connection.timeout_ms,
        } })}>Use Current Connection Settings</button></div>
      <div className="field-grid">
        <label>Baud<input disabled={disabled} type="number" value={draft.serial.baud_rate} onChange={event => updateSerial('baud_rate', event.target.value)} /></label>
        <label>Data bits<select disabled={disabled} value={draft.serial.data_bits} onChange={event => updateSerial('data_bits', event.target.value)}>{[5, 6, 7, 8].map(value => <option key={value}>{value}</option>)}</select></label>
        <label>Parity<select disabled={disabled} value={draft.serial.parity} onChange={event => updateSerial('parity', event.target.value)}>{['none', 'odd', 'even'].map(value => <option key={value}>{value}</option>)}</select></label>
        <label>Stop bits<select disabled={disabled} value={draft.serial.stop_bits} onChange={event => updateSerial('stop_bits', event.target.value)}>{[1, 2].map(value => <option key={value}>{value}</option>)}</select></label>
        <label>Flow control<select disabled={disabled} value={draft.serial.flow_control} onChange={event => updateSerial('flow_control', event.target.value)}>{['none', 'software', 'hardware'].map(value => <option key={value}>{value}</option>)}</select></label>
        <label>Timeout ms<input disabled={disabled} type="number" value={draft.serial.timeout_ms} onChange={event => updateSerial('timeout_ms', event.target.value)} /></label>
      </div>
    </div>
    <div className="editor-columns">
      <section className="panel palette"><h3>Palette</h3>
        <label>Step type<select disabled={disabled} value={addKind} onChange={event => setAddKind(event.target.value as Step['type'])}>
          {kinds.map(kind => <option value={kind} key={kind}>{label(kind)}</option>)}
        </select></label>
        <button disabled={disabled} onClick={() => add(false)}>Add Step</button>
        <button disabled={disabled || step?.type !== 'repeat'} onClick={() => add(true)}>Add Child</button>
      </section>
      <section className="panel steps-panel"><h3>Steps</h3>
        <div className="step-list">{draft.steps.length ? renderSteps(draft.steps) : <p className="muted">Add a step to begin.</p>}</div>
        <div className="toolbar"><button disabled={disabled || !step} onClick={remove}>Delete</button>
          <button disabled={disabled || !step} onClick={() => move(-1)}>Move Up</button>
          <button disabled={disabled || !step} onClick={() => move(1)}>Move Down</button></div>
      </section>
      <section className="panel properties"><h3>Properties</h3>
        {!step ? <p className="muted">Select a step.</p> : <div className="properties-fields">
          <p className="step-kind">{label(step.type)}</p>
          <label>ID<input disabled={disabled} value={step.id} onChange={event => updateStep({ id: event.target.value })} /></label>
          {step.type === 'send_text' && <label>Text<textarea disabled={disabled} value={step.text} onChange={event => updateStep({ text: event.target.value })} /></label>}
          {step.type === 'send_bytes' && <label>Hex<textarea disabled={disabled} value={step.hex} onChange={event => updateStep({ hex: event.target.value })} /></label>}
          {step.type === 'wait' && <label>Duration ms<input disabled={disabled} type="number" value={step.duration_ms} onChange={event => updateStep({ duration_ms: numeric(event.target.value) })} /></label>}
          {(step.type === 'read' || step.type === 'read_until') && <label>Max bytes<input disabled={disabled} type="number" value={step.max_bytes} onChange={event => updateStep({ max_bytes: numeric(event.target.value) })} /></label>}
          {step.type === 'read_until' && <label>Delimiter hex<input disabled={disabled} value={step.delimiter_hex} onChange={event => updateStep({ delimiter_hex: event.target.value })} /></label>}
          {step.type === 'repeat' && <label>Count<input disabled={disabled} type="number" value={step.count} onChange={event => updateStep({ count: numeric(event.target.value) })} /></label>}
        </div>}
      </section>
    </div>
  </div>
}
