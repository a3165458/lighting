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
