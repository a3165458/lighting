import { describe, expect, it } from 'vitest'
import { RequestGate } from './requestGate'

describe('RequestGate', () => {
  it('discards an older refresh after a newer refresh starts', () => {
    const gate = new RequestGate()
    const old = gate.beginRefresh()
    const latest = gate.beginRefresh()

    expect(old).not.toBeNull()
    expect(latest).not.toBeNull()
    expect(gate.isCurrent(old!)).toBe(false)
    expect(gate.isCurrent(latest!)).toBe(true)
  })

  it('blocks polling while a mutation is in flight', () => {
    const gate = new RequestGate()
    const mutation = gate.beginMutation()

    expect(gate.beginRefresh()).toBeNull()
    expect(gate.isCurrent(mutation)).toBe(true)

    gate.finishMutation()
    expect(gate.beginRefresh()).not.toBeNull()
  })

  it('keeps only the latest overlapping mutation current', () => {
    const gate = new RequestGate()
    const first = gate.beginMutation()
    const second = gate.beginMutation()

    expect(gate.isCurrent(first)).toBe(false)
    expect(gate.isCurrent(second)).toBe(true)

    gate.finishMutation()
    expect(gate.beginRefresh()).toBeNull()
    gate.finishMutation()
    expect(gate.beginRefresh()).not.toBeNull()
  })
})
