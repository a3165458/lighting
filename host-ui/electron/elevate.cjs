const { execFile } = require('node:child_process')
const fs = require('node:fs')
const os = require('node:os')
const path = require('node:path')
const { promisify } = require('node:util')

const execFileAsync = promisify(execFile)
const UAC_TIMEOUT_MS = 180_000

function psQuote(value) {
  return `'${String(value).replace(/'/g, "''")}'`
}

const COPY = {
  UAC_CANCELLED: '已取消管理员授权。扩展屏需要安装虚拟显示驱动，请在弹窗中点「是」。',
  UAC_TIMEOUT:
    '等了很久也没有管理员确认窗口。请完全退出 Lighting，右键选「以管理员身份运行」后再点开始共享（已是管理员时不会再弹窗）。',
  ACCESS_DENIED: '权限不足，无法安装虚拟显示驱动。请右键 Lighting 选「以管理员身份运行」。',
  SHELLEXECUTE_FAILED:
    '无法唤起或等待管理员确认窗口。请完全退出 Lighting，右键选「以管理员身份运行」后再试（已是管理员时不会再弹窗）。',
  UAC_HIDDEN_HOST:
    '主机进程没有可见窗口，无法弹出管理员确认。请再点开始共享并留意蓝底「用户账户控制」，或右键以管理员身份运行（已是管理员时不会再弹窗）。',
}

function humanizeElevateCode(code) {
  return COPY[code] || code
}

function mapElevateError(err) {
  const text = `${err?.stderr || ''} ${err?.message || err || ''}`
  if (/1223|canceled|cancelled|被用户取消|操作已取消|UAC_CANCELLED/i.test(text)) {
    return new Error(humanizeElevateCode('UAC_CANCELLED'))
  }
  if (err?.killed || /ETIMEDOUT|timeout|UAC_TIMEOUT/i.test(text)) {
    return new Error(humanizeElevateCode('UAC_TIMEOUT'))
  }
  if (/access is denied|ACCESS_DENIED|0x5\b|拒绝/i.test(text)) {
    return new Error(humanizeElevateCode('ACCESS_DENIED'))
  }
  return new Error(humanizeElevateCode('SHELLEXECUTE_FAILED'))
}

/**
 * Ask for a visible UAC from the Electron/UI process (has a real window),
 * then start lighting-host.exe --ipc-only already elevated so provision.ps1
 * can run in-process. Do not ShellExecute from a windowsHide host.
 */
async function spawnElevatedHost({ hostExe, resourcesDir, port }) {
  const helper = path.join(os.tmpdir(), `lighting-host-runas-${process.pid}-${Date.now()}.ps1`)
  const launcher = path.join(os.tmpdir(), `lighting-uac-${process.pid}-${Date.now()}.ps1`)
  const helperBody = [
    '$ErrorActionPreference = \'Stop\'',
    `$env:LIGHTING_IPC_PORT = ${psQuote(String(port))}`,
    `$env:LIGHTING_RESOURCES_DIR = ${psQuote(resourcesDir)}`,
    `Start-Process -FilePath ${psQuote(hostExe)} -ArgumentList '--ipc-only' -WorkingDirectory ${psQuote(path.dirname(hostExe))} -WindowStyle Hidden`,
    '',
  ].join('\r\n')
  const launcherBody = [
    '$ErrorActionPreference = \'Stop\'',
    'Write-Host \'请在蓝底「用户账户控制」窗口点「是」…\'',
    `$helper = ${psQuote(helper)}`,
    '$p = Start-Process -FilePath "$env:SystemRoot\\System32\\WindowsPowerShell\\v1.0\\powershell.exe" -ArgumentList @(\'-NoProfile\',\'-ExecutionPolicy\',\'Bypass\',\'-File\', $helper) -Verb RunAs -Wait -PassThru',
    'if (-not $p) { throw \'UAC_CANCELLED\' }',
    'if ($null -ne $p.ExitCode -and $p.ExitCode -ne 0) { throw "UAC_HELPER_EXIT:$($p.ExitCode)" }',
    '',
  ].join('\r\n')
  fs.writeFileSync(helper, helperBody, 'utf8')
  fs.writeFileSync(launcher, launcherBody, 'utf8')
  try {
    await execFileAsync(
      'powershell.exe',
      ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', launcher],
      { windowsHide: false, timeout: UAC_TIMEOUT_MS, windowsVerbatimArguments: false },
    )
  } catch (err) {
    throw mapElevateError(err)
  } finally {
    for (const file of [helper, launcher]) {
      try {
        fs.unlinkSync(file)
      } catch {
        /* ignore */
      }
    }
  }
}

module.exports = { spawnElevatedHost, mapElevateError, humanizeElevateCode }
