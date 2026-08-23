package app.lighting.display

import android.content.Intent
import android.os.Bundle
import android.widget.Button
import android.widget.EditText
import android.widget.TextView
import androidx.appcompat.app.AppCompatActivity

class MainActivity : AppCompatActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)
        val capsInfo = findViewById<TextView>(R.id.capsInfo)
        Thread({
            val text = try {
                DeviceCaps.probe().summary()
            } catch (t: Throwable) {
                "解码能力检测失败：${t.message}"
            }
            runOnUiThread { capsInfo.text = text }
        }, "lighting-caps").start()
        val host = findViewById<EditText>(R.id.hostInput)
        val port = findViewById<EditText>(R.id.portInput)
        findViewById<Button>(R.id.connectButton).setOnClickListener {
            val intent = Intent(this, DisplayActivity::class.java)
            intent.putExtra(DisplayActivity.EXTRA_HOST, host.text.toString().ifBlank { "127.0.0.1" })
            intent.putExtra(DisplayActivity.EXTRA_PORT, port.text.toString().toIntOrNull() ?: LitProtocol.PORT)
            startActivity(intent)
        }
    }
}
