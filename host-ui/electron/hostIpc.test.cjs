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
