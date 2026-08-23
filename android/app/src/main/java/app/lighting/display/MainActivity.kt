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
    private lateinit var capsInfo: TextView
    private lateinit var connectError: TextView

    private val displayLauncher = registerForActivityResult(
        ActivityResultContracts.StartActivityForResult(),
    ) { result ->
        val err = result.data?.getStringExtra(DisplayActivity.EXTRA_ERROR)
        if (!err.isNullOrBlank()) {
            showError(err)
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)
        host = findViewById(R.id.hostInput)
        port = findViewById(R.id.portInput)
        capsInfo = findViewById(R.id.capsInfo)
        connectError = findViewById(R.id.connectError)
        applyDefaults()
        Thread({
            val text = try {
                DeviceCaps.probe().summary()
            } catch (t: Throwable) {
                "解码能力检测失败：${t.message ?: t.javaClass.simpleName}"
            }
            runOnUiThread { capsInfo.text = text }
        }, "lighting-caps").start()
        findViewById<Button>(R.id.connectButton).setOnClickListener {
            launchDisplay()
        }
    }

    private fun applyDefaults() {
        if (host.text.isNullOrBlank()) {
            host.setText("127.0.0.1")
        }
        if (port.text.isNullOrBlank()) {
            port.setText(LitProtocol.PORT.toString())
        }
    }

    private fun launchDisplay() {
        applyDefaults()
        val hostText = host.text.toString().trim().ifBlank { "127.0.0.1" }
        val portNum = port.text.toString().trim().toIntOrNull()
        if (portNum == null || portNum !in 1..65535) {
            showError("端口无效，请填 1–65535（默认 ${LitProtocol.PORT}）。")
            return
        }
        hideError()
        try {
            val intent = Intent(this, DisplayActivity::class.java)
            intent.putExtra(DisplayActivity.EXTRA_HOST, hostText)
            intent.putExtra(DisplayActivity.EXTRA_PORT, portNum)
            displayLauncher.launch(intent)
        } catch (t: Throwable) {
            showError("无法打开显示页：${t.message ?: t.javaClass.simpleName}")
        }
    }

    private fun showError(message: String) {
        connectError.text = message
        connectError.visibility = View.VISIBLE
    }

    private fun hideError() {
        connectError.text = ""
        connectError.visibility = View.GONE
    }
}
