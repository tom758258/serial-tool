import test from 'node:test'
import assert from 'node:assert/strict'
import { continuousRxHex, defaultLine, defaultTcpConnection, display, freshSequence, hex, rxTextFragments, sequenceMatches, tcpConnectSettings } from '../src/model.ts'

const rx = (...bytes) => ({ direction: 'rx', bytes })
const tx = (...bytes) => ({ direction: 'tx', bytes })

test('Continuous RX Hex joins consecutive RX events without an event boundary', () => {
  assert.equal(continuousRxHex([rx(0xAA, 0x55), rx(0x00, 0x01), rx(0x0D, 0x0A, 0xFF)]),
    'AA 55 00 01 0D 0A FF')
})

test('Continuous RX Hex ignores TX events even when interleaved with RX', () => {
  assert.equal(continuousRxHex([tx(0x99), rx(0x00), tx(0x7E), rx(0xFF)]), '00 FF')
})

test('Continuous RX Hex preserves every binary byte and uses uppercase padded pairs', () => {
  assert.equal(continuousRxHex([rx(0x00, 0x01, 0x0A, 0x0D, 0x80, 0xAA, 0xFF)]), '00 01 0A 0D 80 AA FF')
})

test('Continuous RX Hex omits empty RX events without redundant separators', () => {
  assert.equal(continuousRxHex([rx(), rx(0x01), rx(), tx(0xBB), rx(0x02), rx()]), '01 02')
  assert.equal(continuousRxHex([rx(), tx(0xDD)]), '')
  assert.equal(continuousRxHex([]), '')
})

test('Continuous RX Hex never inserts newlines for CR or LF bytes', () => {
  assert.equal(continuousRxHex([rx(0x0D), rx(0x0A)]), '0D 0A')
})

test('Continuous RX Hex handles many history entries in their original order', () => {
  const events = Array.from({ length: 5000 }, (_, i) => i % 2 === 0 ? rx(i % 256) : tx(i % 256))
  const result = continuousRxHex(events)
  const parts = result.split(' ')
  assert.equal(parts.length, 2500)
  assert.equal(parts[0], '00')
  assert.equal(parts[1], '02')
  assert.equal(parts.at(-1), hex([4998 % 256]))
})

test('legacy continuous text concatenates RX and retains CR/LF behavior', () => {
  const events = [rx(0x48, 0x65, 0x6C), tx(0xFF), rx(0x6C, 0x6F, 0x0D), rx(0x0A, 0x21)]
  assert.equal(rxTextFragments(events, true).join(''), 'Hello\n!')
})

test('legacy continuous text decodes UTF-8 that spans multiple RX events', () => {
  assert.equal(rxTextFragments([rx(0xE4, 0xB8), tx(0xAA), rx(0xAD)], true).join(''), '中')
})

test('legacy continuous text continues escaping non-newline control characters', () => {
  assert.equal(rxTextFragments([rx(0x00, 0x09, 0x7F)], true).join(''), '\\u{0}\\t\\u{7F}')
})

test('existing event presentation still displays hex, text and both', () => {
  assert.equal(display([0x4F, 0x4B], 'hex'), '4F 4B')
  assert.equal(display([0x4F, 0x4B], 'text'), 'OK')
  assert.equal(display([0x4F, 0x4B], 'both'), '4F 4B  |  OK')
  assert.deepEqual(rxTextFragments([rx(0x0D, 0x0A)], false), ['\\r\\n'])
})

test('TCP Connect parses live settings and rejects invalid host, port, or timeouts', () => {
  assert.deepEqual(tcpConnectSettings({ host: '127.0.0.1', port: '502', connect_timeout_ms: '3000', io_timeout_ms: '1000' }, 'live'),
    { host: '127.0.0.1', port: 502, connect_timeout_ms: 3000, io_timeout_ms: 1000 })
  for (const patch of [
    { host: '' }, { host: 'http://example.com' }, { port: '' }, { port: '0' },
    { port: '65536' }, { port: '12x' }, { connect_timeout_ms: '0' }, { io_timeout_ms: '-10' },
  ]) {
    assert.throws(() => tcpConnectSettings({ ...defaultTcpConnection, host: 'localhost', port: '42', ...patch }, 'live'))
  }
})

test('TCP Simulation ignores Host and Port but still validates timeouts', () => {
  assert.deepEqual(tcpConnectSettings({
    host: 'http://ignored.invalid', port: 'not-a-port', connect_timeout_ms: '3000', io_timeout_ms: '1000',
  }, 'simulation'), { host: '', port: 0, connect_timeout_ms: 3000, io_timeout_ms: 1000 })
  assert.throws(() => tcpConnectSettings({ ...defaultTcpConnection, io_timeout_ms: '0' }, 'simulation'))
})

test('Sequence New and matching are transport-specific', () => {
  const serial = freshSequence(defaultLine)
  const tcp = freshSequence(defaultLine, 'tcp', 1000)
  assert.equal(serial.transport.type, 'serial')
  assert.deepEqual(tcp.transport, { type: 'tcp', settings: { io_timeout_ms: 1000 } })
  const tcpConnection = { ...defaultTcpConnection, io_timeout_ms: '1000' }
  assert.equal(sequenceMatches(serial, 'serial', defaultLine, tcpConnection), true)
  assert.equal(sequenceMatches(serial, 'tcp', defaultLine, tcpConnection), false)
  assert.equal(sequenceMatches(tcp, 'serial', defaultLine, tcpConnection), false)
  assert.equal(sequenceMatches(tcp, 'tcp', defaultLine, tcpConnection), true)
  assert.equal(sequenceMatches(tcp, 'tcp', defaultLine, { ...tcpConnection, io_timeout_ms: '2000' }), false)
  assert.equal(sequenceMatches(serial, 'serial', { ...defaultLine, baud_rate: 9600 }, tcpConnection), false)
})
