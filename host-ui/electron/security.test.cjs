const test = require('node:test')
const assert = require('node:assert/strict')

const { isSafeExternalUrl } = require('./security.cjs')

test('only web URLs can be opened outside the app', () => {
  assert.equal(isSafeExternalUrl('https://lighting.example/help'), true)
  assert.equal(isSafeExternalUrl('http://127.0.0.1/help'), true)
  assert.equal(isSafeExternalUrl('file:///C:/Windows/System32/calc.exe'), false)
  assert.equal(isSafeExternalUrl('javascript:alert(1)'), false)
  assert.equal(isSafeExternalUrl('not a URL'), false)
})
