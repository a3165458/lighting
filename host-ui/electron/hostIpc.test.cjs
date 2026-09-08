const test = require('node:test')
const assert = require('node:assert/strict')

const { HostIpcClient } = require('./hostIpc.cjs')

test('RPC requests time out and release their pending entry', async () => {
  const client = new HostIpcClient({ requestTimeoutMs: 10 })
  client.socket = {
    write(_payload, callback) {
      callback(null)
    },
  }

  const outcome = await Promise.race([
    client.rawInvoke('getState').then(
      () => 'resolved',
      (error) => error,
    ),
    new Promise((resolve) => setTimeout(() => resolve('still-pending'), 50)),
  ])

  assert.notEqual(outcome, 'still-pending')
  assert.match(String(outcome?.message || outcome), /timeout|超时/i)
  assert.equal(client.pending.size, 0)
})

test('RPC requests include the per-process authentication token', async () => {
  const client = new HostIpcClient({ requestTimeoutMs: 100, token: 'test-secret' })
  let written = ''
  client.socket = {
    write(payload, callback) {
      written = payload
      callback(null)
    },
  }

  const pending = client.rawInvoke('ping')
  const request = JSON.parse(written)
  client.onData(`${JSON.stringify({ id: request.id, ok: true, result: {} })}\n`)
  await pending

  assert.equal(request.token, 'test-secret')
})

test('concurrent reconnects share one host startup attempt', async () => {
  const client = new HostIpcClient()
  let starts = 0
  let connects = 0
  client.startHostIfNeeded = async () => {
    starts += 1
    await new Promise((resolve) => setTimeout(resolve, 10))
  }
  client.connectWithRetry = async () => {
    connects += 1
    client.socket = {}
    client.connected = true
  }
  client.alignHostVersion = async () => {}

  await Promise.all([client.ensureConnected(), client.ensureConnected()])

  assert.equal(starts, 1)
  assert.equal(connects, 1)
})

test('an unterminated oversized host response is rejected', async () => {
  const client = new HostIpcClient({ requestTimeoutMs: 200 })
  let destroyed = false
  client.socket = {
    write(_payload, callback) {
      callback(null)
    },
    destroy() {
      destroyed = true
    },
  }
  const pending = client.rawInvoke('getState')

  client.onData('x'.repeat(1024 * 1024 + 1))

  await assert.rejects(pending, /response|响应|large|过大/i)
  assert.equal(destroyed, true)
  assert.equal(client.pending.size, 0)
})
