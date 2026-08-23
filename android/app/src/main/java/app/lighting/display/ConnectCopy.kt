package app.lighting.display

import java.io.EOFException
import java.net.ConnectException
import java.net.Inet4Address
import java.net.NetworkInterface
import java.net.NoRouteToHostException
import java.net.SocketTimeoutException
import java.net.UnknownHostException

/**
 * Beginner-facing copy. LIT1 still uses [LitProtocol.PORT] internally;
 * primary lines never mention ports or raw socket errors.
 */
data class UserFacingError(
    val primary: String,
    val hint: String,
    val detail: String,
)

object ConnectCopy {
    const val USB_HOST = "127.0.0.1"

    fun isUsbHost(host: String): Boolean {
        val h = host.trim().lowercase()
        return h.isEmpty() ||
            h == USB_HOST ||
            h == "localhost" ||
            h == "::1" ||
            h == "[::1]"
    }

    fun connectingLabel(host: String): String =
        if (isUsbHost(host)) "正在通过 USB 连接…" else "正在连接电脑…"

    /** Shown on the waiting screen so the user can compare it with the PC. */
    fun localAddressLabel(): String = try {
        NetworkInterface.getNetworkInterfaces()
            ?.toList()
            ?.asSequence()
            ?.filter { it.isUp && !it.isLoopback }
            ?.flatMap { it.inetAddresses.toList().asSequence() }
            ?.filterIsInstance<Inet4Address>()
            ?.mapNotNull { it.hostAddress }
            ?.firstOrNull { !it.startsWith("127.") }
            ?: "未连接网络（USB 仍可用）"
    } catch (_: Throwable) {
        "未连接网络（USB 仍可用）"
    }

    fun deviceLabel(caps: DeviceCaps): String =
        listOf(caps.manufacturer, caps.model)
            .filter { it.isNotBlank() }
            .joinToString(" ")
            .ifBlank { "本机" }

    fun capsCard(caps: DeviceCaps): String {
        val title = deviceLabel(caps)
        val avc = if (caps.avc != null) "AVC 硬解" else "AVC 软解"
        val hevc = if (caps.hevc != null) "HEVC 硬解" else "HEVC 无硬解"
        return "$title\n$avc · $hevc"
    }

    fun fromThrowable(error: Throwable, host: String): UserFacingError {
        val detail = technicalDetail(error)
        val blob = errorBlob(error)
        val usb = isUsbHost(host)
        val mapped = when {
            isUnknownHost(error, blob) ->
                if (usb) {
                    usbFailure(
                        "没检测到电脑，请检查数据线是否支持传数据",
                        "请打开 USB 调试并点允许，并先在电脑点开始共享。",
                        detail,
                    )
                } else {
                    UserFacingError("找不到电脑，请在高级里确认地址", "", detail)
                }
            isRefused(error, blob) ->
                if (usb) {
                    usbFailure(
                        "请先在电脑点开始共享",
                        "也可以检查数据线是否支持传数据，并打开 USB 调试后点允许。",
                        detail,
                    )
                } else {
                    UserFacingError("请先在电脑点开始共享", "", detail)
                }
            isTimeout(error, blob) || isUnreachable(error, blob) ->
                if (usb) {
                    usbFailure(
                        "没检测到电脑，请检查数据线是否支持传数据",
                        "请打开 USB 调试并点允许，并先在电脑点开始共享。",
                        detail,
                    )
                } else {
                    UserFacingError("连不上电脑，请确认在同一网络", "", detail)
                }
            isResetOrEof(error, blob) ->
                UserFacingError(
                    "连接中断，请检查数据线或重新点开始",
                    if (usb) "请打开 USB 调试并点允许。" else "",
                    detail,
                )
            usb ->
                usbFailure(
                    "请打开 USB 调试并点允许",
                    "请换一根能传数据的线，并先在电脑点开始共享。",
                    detail,
                )
            else -> UserFacingError("连接失败，请稍后重试", "", detail)
        }
        return mapped.copy(
            primary = stripPortJargon(mapped.primary),
            hint = stripPortJargon(mapped.hint),
        )
    }

    private fun stripPortJargon(text: String): String {
        if (!hasPortJargon(text)) return text
        return "没检测到电脑，请检查数据线是否支持传数据"
    }

    fun hasPortJargon(text: String): Boolean {
        val blob = text.lowercase()
        return blob.contains("17400") ||
            blob.contains("connection refused") ||
            blob.contains("econnrefused") ||
            blob.contains("adb reverse") ||
            Regex("""\b(?:127\.0\.0\.1|0\.0\.0\.0|localhost):\d+""").containsMatchIn(blob)
    }

    private fun usbFailure(primary: String, hint: String, detail: String): UserFacingError =
        UserFacingError(primary, hint, detail)

    private fun technicalDetail(error: Throwable): String {
        val parts = ArrayList<String>()
        var t: Throwable? = error
        var depth = 0
        while (t != null && depth < 4) {
            val msg = t.message?.trim().orEmpty()
            val line = if (msg.isEmpty()) t.javaClass.simpleName else "${t.javaClass.simpleName}: $msg"
            if (parts.none { it == line }) {
                parts.add(line)
            }
            t = t.cause
            depth++
        }
        return parts.joinToString("\n")
    }

    private fun errorBlob(error: Throwable): String {
        val out = StringBuilder()
        var t: Throwable? = error
        while (t != null) {
            out.append(t.javaClass.simpleName).append(' ')
            out.append(t.message.orEmpty()).append(' ')
            t = t.cause
        }
        return out.toString().lowercase()
    }

    private fun isUnknownHost(error: Throwable, blob: String): Boolean =
        error is UnknownHostException || "unknownhost" in blob || "unresolved" in blob

    private fun isRefused(error: Throwable, blob: String): Boolean =
        error is ConnectException || "refused" in blob || "econnrefused" in blob

    private fun isTimeout(error: Throwable, blob: String): Boolean =
        error is SocketTimeoutException || "timed out" in blob || "timeout" in blob

    private fun isUnreachable(error: Throwable, blob: String): Boolean =
        error is NoRouteToHostException || "unreachable" in blob || "no route" in blob

    private fun isResetOrEof(error: Throwable, blob: String): Boolean =
        error is EOFException ||
            isEof(error) ||
            "econnreset" in blob ||
            "broken pipe" in blob ||
            ("reset" in blob && "refused" !in blob)
}
