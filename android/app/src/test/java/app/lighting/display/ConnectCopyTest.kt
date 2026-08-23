package app.lighting.display

import java.io.EOFException
import java.net.ConnectException
import java.util.Calendar
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ConnectCopyTest {
    @Test
    fun errors_never_mention_ports_or_sockets() {
        val refused = ConnectCopy.fromThrowable(
            ConnectException("failed to connect to /127.0.0.1 (port 17400): ECONNREFUSED"),
            ConnectCopy.USB_HOST,
        )
        assertEquals("请先在电脑点开始共享", refused.primary)
        assertFalse(ConnectCopy.hasPortJargon(refused.primary))
        assertFalse(ConnectCopy.hasPortJargon(refused.hint))
        assertTrue(refused.detail.contains("ConnectException"))
    }

    @Test
    fun dropped_stream_asks_about_the_cable() {
        val reset = ConnectCopy.fromThrowable(EOFException(), ConnectCopy.USB_HOST)
        assertEquals("连接中断，请检查数据线或重新点开始", reset.primary)
    }

    @Test
    fun lan_failures_point_at_the_address_not_the_port() {
        val lan = ConnectCopy.fromThrowable(
            ConnectException("connect refused to 192.168.1.100:17400"),
            "192.168.1.100",
        )
        assertEquals("请先在电脑点开始共享", lan.primary)
        assertFalse(ConnectCopy.hasPortJargon(lan.primary))
    }

    @Test
    fun port_jargon_detector_catches_the_usual_leaks() {
        assertTrue(ConnectCopy.hasPortJargon("connect to 127.0.0.1:17400 failed"))
        assertTrue(ConnectCopy.hasPortJargon("ECONNREFUSED"))
        assertTrue(ConnectCopy.hasPortJargon("adb reverse failed"))
        assertFalse(ConnectCopy.hasPortJargon("请先在电脑点开始共享"))
    }

    @Test
    fun usb_host_recognized_in_every_spelling() {
        assertTrue(ConnectCopy.isUsbHost(""))
        assertTrue(ConnectCopy.isUsbHost("127.0.0.1"))
        assertTrue(ConnectCopy.isUsbHost("localhost"))
        assertTrue(ConnectCopy.isUsbHost("::1"))
        assertFalse(ConnectCopy.isUsbHost("192.168.1.100"))
    }

    @Test
    fun connecting_label_matches_the_transport() {
        assertEquals("正在通过 USB 连接…", ConnectCopy.connectingLabel(ConnectCopy.USB_HOST))
        assertEquals("正在连接电脑…", ConnectCopy.connectingLabel("192.168.1.100"))
    }

    @Test
    fun backoff_grows_and_stays_jittered() {
        assertEquals(650L, reconnectBackoffMs(0, 0))
        assertEquals(850L, reconnectBackoffMs(0, 999))
        assertTrue(reconnectBackoffMs(4, 0) > reconnectBackoffMs(0, 0))
    }
}

class ConnectHistoryTest {
    private fun at(daysAgo: Int, hour: Int, minute: Int): Pair<Long, Long> {
        val now = Calendar.getInstance().apply {
            set(Calendar.HOUR_OF_DAY, 20)
            set(Calendar.MINUTE, 0)
            set(Calendar.SECOND, 0)
            set(Calendar.MILLISECOND, 0)
        }
        val then = (now.clone() as Calendar).apply {
            add(Calendar.DAY_OF_YEAR, -daysAgo)
            set(Calendar.HOUR_OF_DAY, hour)
            set(Calendar.MINUTE, minute)
        }
        return then.timeInMillis to now.timeInMillis
    }

    @Test
    fun last_seen_reads_like_a_person_wrote_it() {
        val (today, now) = at(0, 9, 45)
        assertEquals("上次连接：今天 09:45", ConnectHistory.lastSeenLabel(today, now))
        val (yesterday, now2) = at(1, 18, 22)
        assertEquals("上次连接：昨天 18:22", ConnectHistory.lastSeenLabel(yesterday, now2))
        val (threeDays, now3) = at(3, 12, 0)
        assertEquals("上次连接：3 天前", ConnectHistory.lastSeenLabel(threeDays, now3))
        assertEquals("上次连接：—", ConnectHistory.lastSeenLabel(0L, now))
    }

    @Test
    fun usb_rows_hide_the_loopback_address() {
        val usb = HistoryEntry("DESKTOP-7G4K2M1", "127.0.0.1", LitProtocol.PORT, 1L)
        assertEquals("USB 直连", ConnectHistory.displayHost(usb))
        assertEquals("DESKTOP-7G4K2M1", ConnectHistory.displayName(usb))

        val nameless = HistoryEntry("", "127.0.0.1", LitProtocol.PORT, 1L)
        assertEquals("USB 连接的电脑", ConnectHistory.displayName(nameless))

        val lan = HistoryEntry("", "192.168.1.100", LitProtocol.PORT, 1L)
        assertEquals("192.168.1.100", ConnectHistory.displayHost(lan))
        assertEquals("电脑", ConnectHistory.displayName(lan))
    }
}
