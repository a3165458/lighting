package app.lighting.display

import android.content.Intent
import android.os.Bundle
import android.view.View
import android.widget.Button
import android.widget.EditText
import android.widget.TextView
import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AppCompatActivity

class MainActivity : AppCompatActivity() {
    private lateinit var host: EditText
    private lateinit var port: EditText
    private lateinit var advancedToggle: TextView
    private lateinit var advancedPanel: View
    private lateinit var errorBox: View
    private lateinit var connectError: TextView
    private lateinit var errorHint: TextView
    private lateinit var errorDetailToggle: TextView
    private lateinit var errorDetail: TextView
    private lateinit var capsInfo: TextView

    private var advancedOpen = false
    private var detailOpen = false

    private val displayLauncher = registerForActivityResult(
        ActivityResultContracts.StartActivityForResult(),
    ) { result ->
        val primary = result.data?.getStringExtra(DisplayActivity.EXTRA_ERROR)
        if (!primary.isNullOrBlank()) {
            val hint = result.data?.getStringExtra(DisplayActivity.EXTRA_ERROR_HINT).orEmpty()
            val detail = result.data?.getStringExtra(DisplayActivity.EXTRA_ERROR_DETAIL).orEmpty()
            showError(UserFacingError(primary, hint, detail))
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)
        host = findViewById(R.id.hostInput)
        port = findViewById(R.id.portInput)
        advancedToggle = findViewById(R.id.advancedToggle)
        advancedPanel = findViewById(R.id.advancedPanel)
        errorBox = findViewById(R.id.errorBox)
        connectError = findViewById(R.id.connectError)
        errorHint = findViewById(R.id.errorHint)
        errorDetailToggle = findViewById(R.id.errorDetailToggle)
        errorDetail = findViewById(R.id.errorDetail)
        capsInfo = findViewById(R.id.capsInfo)

        applyAdvancedHints()
        renderAdvanced()
        hideError()

        advancedToggle.setOnClickListener {
            advancedOpen = !advancedOpen
            renderAdvanced()
        }
        errorDetailToggle.setOnClickListener {
            detailOpen = !detailOpen
            renderErrorDetail()
        }

        findViewById<Button>(R.id.connectButton).setOnClickListener {
            launchDisplay(ConnectCopy.USB_HOST, LitProtocol.PORT)
        }
        findViewById<Button>(R.id.lanConnectButton).setOnClickListener {
            launchLan()
        }

        Thread({
            val text = try {
                ConnectCopy.capsCard(DeviceCaps.probe())
            } catch (t: Throwable) {
                "本机解码能力暂不可用"
            }
            runOnUiThread { capsInfo.text = text }
        }, "lighting-caps").start()
    }

    private fun applyAdvancedHints() {
        host.hint = "电脑的局域网 IP"
        port.hint = LitProtocol.PORT.toString()
        if (port.text.isNullOrBlank()) {
            port.setText(LitProtocol.PORT.toString())
        }
    }

    private fun renderAdvanced() {
        advancedPanel.visibility = if (advancedOpen) View.VISIBLE else View.GONE
        advancedToggle.text = if (advancedOpen) "高级 ▾" else "高级 ▸"
    }

    private fun launchLan() {
        applyAdvancedHints()
        val hostText = host.text.toString().trim()
        if (hostText.isEmpty() || ConnectCopy.isUsbHost(hostText)) {
            showError(
                UserFacingError(
                    "请填写电脑的局域网 IP",
                    "USB 请直接点上面的「USB 一键连接」。",
                    "",
                ),
            )
            advancedOpen = true
            renderAdvanced()
            return
        }
        val portNum = port.text.toString().trim().toIntOrNull()
        if (portNum == null || portNum !in 1..65535) {
            showError(
                UserFacingError(
                    "高级端口无效，请核对后再试",
                    "",
                    port.text.toString(),
                ),
            )
            return
        }
        launchDisplay(hostText, portNum)
    }

    private fun launchDisplay(hostText: String, portNum: Int) {
        hideError()
        try {
            val intent = Intent(this, DisplayActivity::class.java)
            intent.putExtra(DisplayActivity.EXTRA_HOST, hostText)
            intent.putExtra(DisplayActivity.EXTRA_PORT, portNum)
            displayLauncher.launch(intent)
        } catch (t: Throwable) {
            showError(ConnectCopy.fromThrowable(t, hostText))
        }
    }

    private fun showError(error: UserFacingError) {
        val primary = error.primary.ifBlank { "连接失败，请稍后重试" }.let { text ->
            if (ConnectCopy.hasPortJargon(text)) {
                "没检测到电脑，请检查数据线是否支持传数据"
            } else {
                text
            }
        }
        connectError.text = primary
        errorHint.text = error.hint
        errorHint.visibility = if (error.hint.isBlank()) View.GONE else View.VISIBLE
        errorDetail.text = error.detail
        errorBox.visibility = View.VISIBLE
        detailOpen = false
        errorDetailToggle.visibility = if (error.detail.isBlank()) View.GONE else View.VISIBLE
        renderErrorDetail()
    }

    private fun hideError() {
        errorBox.visibility = View.GONE
        connectError.text = ""
        errorHint.text = ""
        errorDetail.text = ""
        detailOpen = false
    }

    private fun renderErrorDetail() {
        errorDetail.visibility = if (detailOpen && errorDetail.text.isNotBlank()) View.VISIBLE else View.GONE
        errorDetailToggle.text = if (detailOpen) "详情 ▾" else "详情 ▸"
    }
}
