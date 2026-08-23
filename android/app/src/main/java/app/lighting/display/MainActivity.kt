package app.lighting.display

import android.content.Intent
import android.os.Build
import android.os.Bundle
import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import android.widget.EditText
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.TextView
import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AppCompatActivity
import com.google.android.material.button.MaterialButton

class MainActivity : AppCompatActivity() {
    private lateinit var host: EditText
    private lateinit var port: EditText
    private lateinit var settingsButton: ImageView
    private lateinit var advancedPanel: View
    private lateinit var helpToggle: TextView
    private lateinit var helpPanel: View
    private lateinit var errorBox: View
    private lateinit var connectError: TextView
    private lateinit var errorHint: TextView
    private lateinit var errorDetailToggle: TextView
    private lateinit var errorDetail: TextView
    private lateinit var capsInfo: TextView
    private lateinit var deviceName: TextView
    private lateinit var localAddress: TextView
    private lateinit var pulse: PulseRingView
    private lateinit var historyList: LinearLayout
    private lateinit var historyEmpty: TextView

    private var advancedOpen = false
    private var helpOpen = false
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
        renderHistory()
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)
        host = findViewById(R.id.hostInput)
        port = findViewById(R.id.portInput)
        settingsButton = findViewById(R.id.settingsButton)
        advancedPanel = findViewById(R.id.advancedPanel)
        helpToggle = findViewById(R.id.helpToggle)
        helpPanel = findViewById(R.id.helpPanel)
        errorBox = findViewById(R.id.errorBox)
        connectError = findViewById(R.id.connectError)
        errorHint = findViewById(R.id.errorHint)
        errorDetailToggle = findViewById(R.id.errorDetailToggle)
        errorDetail = findViewById(R.id.errorDetail)
        capsInfo = findViewById(R.id.capsInfo)
        deviceName = findViewById(R.id.deviceName)
        localAddress = findViewById(R.id.localAddress)
        pulse = findViewById(R.id.pulse)
        historyList = findViewById(R.id.historyList)
        historyEmpty = findViewById(R.id.historyEmpty)

        applyAdvancedHints()
        renderAdvanced()
        renderHelp()
        hideError()

        deviceName.text = "${Build.MANUFACTURER} ${Build.MODEL}".trim().ifBlank { "本机" }

        settingsButton.setOnClickListener {
            advancedOpen = !advancedOpen
            renderAdvanced()
        }
        helpToggle.setOnClickListener {
            helpOpen = !helpOpen
            renderHelp()
        }
        errorDetailToggle.setOnClickListener {
            detailOpen = !detailOpen
            renderErrorDetail()
        }

        val connect = { launchDisplay(ConnectCopy.USB_HOST, LitProtocol.PORT) }
        findViewById<MaterialButton>(R.id.connectButton).setOnClickListener { connect() }
        pulse.setOnClickListener { connect() }
        findViewById<MaterialButton>(R.id.lanConnectButton).setOnClickListener { launchLan() }

        Thread({
            val caps = try {
                ConnectCopy.capsCard(DeviceCaps.probe())
            } catch (t: Throwable) {
                "本机解码能力暂不可用"
            }
            val address = ConnectCopy.localAddressLabel()
            runOnUiThread {
                capsInfo.text = caps
                localAddress.text = address
            }
        }, "lighting-caps").start()
    }

    override fun onResume() {
        super.onResume()
        pulse.pulsing = true
        renderHistory()
    }

    override fun onPause() {
        pulse.pulsing = false
        super.onPause()
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
    }

    private fun renderHelp() {
        helpPanel.visibility = if (helpOpen) View.VISIBLE else View.GONE
        helpToggle.text = if (helpOpen) "收起帮助文档" else "查看帮助文档"
    }

    private fun renderHistory() {
        val entries = ConnectHistory.load(this)
        historyList.removeAllViews()
        historyEmpty.visibility = if (entries.isEmpty()) View.VISIBLE else View.GONE
        val inflater = LayoutInflater.from(this)
        entries.forEach { entry ->
            val row = inflater.inflate(R.layout.item_history, historyList, false)
            row.findViewById<TextView>(R.id.historyName).text = ConnectHistory.displayName(entry)
            row.findViewById<TextView>(R.id.historyHost).text = ConnectHistory.displayHost(entry)
            row.findViewById<TextView>(R.id.historyLastSeen).text =
                ConnectHistory.lastSeenLabel(entry.lastMs)
            row.setOnClickListener { launchDisplay(entry.host, entry.port) }
            historyList.addView(row, ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT)
        }
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
