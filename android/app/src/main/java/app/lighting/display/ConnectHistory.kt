package app.lighting.display

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject
import java.util.Calendar

data class HistoryEntry(
    val name: String,
    val host: String,
    val port: Int,
    val lastMs: Long,
)

/**
 * Remembers the PCs this tablet has streamed from so the home screen can offer
 * "tap to reconnect" instead of asking beginners to retype an address.
 */
object ConnectHistory {
    private const val PREFS = "lighting_history"
    private const val KEY = "entries"
    private const val LIMIT = 6

    fun load(context: Context): List<HistoryEntry> {
        val raw = prefs(context).getString(KEY, null) ?: return emptyList()
        val out = ArrayList<HistoryEntry>()
        try {
            val arr = JSONArray(raw)
            for (i in 0 until arr.length()) {
                val o = arr.optJSONObject(i) ?: continue
                val host = o.optString("host").trim()
                if (host.isEmpty()) continue
                out.add(
                    HistoryEntry(
                        name = o.optString("name").trim(),
                        host = host,
                        port = o.optInt("port", LitProtocol.PORT),
                        lastMs = o.optLong("lastMs", 0L),
                    ),
                )
            }
        } catch (_: Exception) {
            return emptyList()
        }
        return out.sortedByDescending { it.lastMs }.take(LIMIT)
    }

    fun remember(context: Context, name: String, host: String, port: Int, nowMs: Long = System.currentTimeMillis()) {
        val key = historyKey(host, port)
        val kept = load(context).filter { historyKey(it.host, it.port) != key }
        val entry = HistoryEntry(name.trim(), host.trim(), port, nowMs)
        val merged = (listOf(entry) + kept).take(LIMIT)
        val arr = JSONArray()
        merged.forEach {
            arr.put(
                JSONObject()
                    .put("name", it.name)
                    .put("host", it.host)
                    .put("port", it.port)
                    .put("lastMs", it.lastMs),
            )
        }
        prefs(context).edit().putString(KEY, arr.toString()).apply()
    }

    /** USB sessions all share one loopback address, so they collapse into one row. */
    private fun historyKey(host: String, port: Int): String =
        if (ConnectCopy.isUsbHost(host)) "usb" else "$host:$port"

    fun displayName(entry: HistoryEntry): String = when {
        entry.name.isNotBlank() -> entry.name
        ConnectCopy.isUsbHost(entry.host) -> "USB 连接的电脑"
        else -> "电脑"
    }

    fun displayHost(entry: HistoryEntry): String =
        if (ConnectCopy.isUsbHost(entry.host)) "USB 直连" else entry.host

    fun lastSeenLabel(lastMs: Long, nowMs: Long = System.currentTimeMillis()): String {
        if (lastMs <= 0L) return "上次连接：—"
        val then = Calendar.getInstance().apply { timeInMillis = lastMs }
        val now = Calendar.getInstance().apply { timeInMillis = nowMs }
        val clock = String.format(
            "%02d:%02d",
            then.get(Calendar.HOUR_OF_DAY),
            then.get(Calendar.MINUTE),
        )
        val days = dayIndex(now) - dayIndex(then)
        return when {
            days <= 0L -> "上次连接：今天 $clock"
            days == 1L -> "上次连接：昨天 $clock"
            days < 7L -> "上次连接：$days 天前"
            else -> "上次连接：${then.get(Calendar.MONTH) + 1} 月 ${then.get(Calendar.DAY_OF_MONTH)} 日"
        }
    }

    private fun dayIndex(cal: Calendar): Long {
        val copy = cal.clone() as Calendar
        copy.set(Calendar.HOUR_OF_DAY, 0)
        copy.set(Calendar.MINUTE, 0)
        copy.set(Calendar.SECOND, 0)
        copy.set(Calendar.MILLISECOND, 0)
        return copy.timeInMillis / 86_400_000L
    }

    private fun prefs(context: Context) =
        context.applicationContext.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
}
