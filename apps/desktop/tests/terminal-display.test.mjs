import test from 'node:test'
import assert from 'node:assert/strict'
import { continuousRxHex, display, hex, rxTextFragments } from '../src/model.ts'

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
