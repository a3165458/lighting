package app.lighting.display

import android.media.MediaCodecInfo
import android.media.MediaCodecList
import android.media.MediaFormat
import android.os.Build
import android.util.Log

/**
 * Per-SoC decode limits so Qualcomm / MediaTek / Exynos / HiSilicon / Unisoc
 * / Amlogic devices get a stream they can actually hardware-decode.
 */
data class DeviceCaps(
    val brand: String,
    val manufacturer: String,
    val model: String,
    val hardware: String,
    val soc: String,
    val gsi: Boolean,
    val codecs: List<String>,
    val decoderMaxWidth: Int,
    val decoderMaxHeight: Int,
    val decoderMaxFps: Int,
    val hwDecode: Boolean,
    val alignment: Int,
    val lowLatencySafe: Boolean,
    val avc: CodecLimit?,
    val hevc: CodecLimit?,
) {
    fun summary(): String {
        val title = listOf(manufacturer, model).filter { it.isNotBlank() }.joinToString(" ").ifBlank { "本机" }
        val chip = socDisplayName() + if (gsi) " · GSI" else ""
        val avcLine = avc?.let { "AVC    ${it.width}×${it.height}@${it.fps} 硬解" }
            ?: "AVC    软解 ${decoderMaxWidth}×${decoderMaxHeight}@${decoderMaxFps}"
        val hevcLine = hevc?.let { "HEVC  ${it.width}×${it.height}@${it.fps} 硬解" }
            ?: "HEVC  无硬解"
        return "$title\n芯片   $chip\n$avcLine\n$hevcLine"
    }

    fun socDisplayName(): String = when (soc) {
        "qcom" -> "高通"
        "mtk" -> "联发科"
        "exynos" -> "Exynos"
        "hisi" -> "麒麟"
        "tensor" -> "Tensor"
        "unisoc" -> "紫光展锐"
        "amlogic" -> "Amlogic"
        "rockchip" -> "瑞芯微"
        "allwinner" -> "全志"
        "unknown" -> "未知"
        else -> soc
    }

    data class CodecLimit(
        val width: Int,
        val height: Int,
        val fps: Int,
        val hw: Boolean,
        val name: String,
    )
    companion object {
        private const val TAG = "LightingCaps"
        @Volatile private var cached: DeviceCaps? = null

        fun probe(): DeviceCaps {
            cached?.let { return it }
            synchronized(this) {
                cached?.let { return it }
                val caps = measure()
                cached = caps
                Log.i(TAG, "caps=$caps")
                return caps
            }
        }

        private fun measure(): DeviceCaps {
            val brand = Build.BRAND.orEmpty()
            val manufacturer = Build.MANUFACTURER.orEmpty()
            val model = Build.MODEL.orEmpty()
            val hardware = Build.HARDWARE.orEmpty()
            val fingerprint = Build.FINGERPRINT.orEmpty().lowercase()
            val gsi = fingerprint.contains("gsi") ||
                fingerprint.contains("trebledroid") ||
                fingerprint.contains("tdgsi") ||
                model.contains("TrebleDroid", true) ||
                model.contains("GSI", true)

            val avc = bestHardware(MediaFormat.MIMETYPE_VIDEO_AVC)
            val hevc = bestHardware(MediaFormat.MIMETYPE_VIDEO_HEVC)
            val soc = detectSoc(hardware, avc?.name, hevc?.name)
            val codecs = ArrayList<String>()
            if (hasDecoder(MediaFormat.MIMETYPE_VIDEO_AVC)) codecs.add("avc")
            if (hasDecoder(MediaFormat.MIMETYPE_VIDEO_HEVC)) codecs.add("hevc")
            if (codecs.isEmpty()) codecs.add("avc")

            val primary = avc ?: hevc
            val maxW: Int
            val maxH: Int
            val maxFps: Int
            val align: Int
            val hw: Boolean
            if (primary != null) {
                maxW = primary.maxW
                maxH = primary.maxH
                maxFps = primary.maxFps
                align = primary.align
                hw = true
            } else {
                maxW = 1280
                maxH = 720
                maxFps = 30
                align = 2
                hw = false
            }

            val lowLatencySafe = when (soc) {
                "qcom", "hisi", "unisoc" -> !gsi
                "mtk", "exynos", "tensor" -> true
                else -> !gsi
            }

            return DeviceCaps(
                brand = brand,
                manufacturer = manufacturer,
                model = model,
                hardware = hardware,
                soc = soc,
                gsi = gsi,
                codecs = codecs,
                decoderMaxWidth = maxW,
                decoderMaxHeight = maxH,
                decoderMaxFps = maxFps.coerceIn(24, 120),
                hwDecode = hw,
                alignment = align.coerceAtLeast(2),
                lowLatencySafe = lowLatencySafe,
                avc = avc?.let { DeviceCaps.CodecLimit(it.maxW, it.maxH, it.maxFps.coerceIn(24, 120), true, it.name) },
                hevc = hevc?.let { DeviceCaps.CodecLimit(it.maxW, it.maxH, it.maxFps.coerceIn(24, 120), true, it.name) },
            )
        }

        private data class HwLimit(
            val name: String,
            val maxW: Int,
            val maxH: Int,
            val maxFps: Int,
            val align: Int,
        )

        private fun bestHardware(mime: String): HwLimit? {
            var best: HwLimit? = null
            val list = MediaCodecList(MediaCodecList.ALL_CODECS)
            for (info in list.codecInfos) {
                if (info.isEncoder) continue
                if (info.supportedTypes.none { it.equals(mime, true) }) continue
                val name = info.name
                if (isSoftwareName(info, name)) continue
                val limit = try {
                    readLimit(info, mime, name)
                } catch (_: Throwable) {
                    continue
                } ?: continue
                if (best == null || limit.maxW.toLong() * limit.maxH > best.maxW.toLong() * best.maxH) {
                    best = limit
                } else if (limit.maxW.toLong() * limit.maxH == best.maxW.toLong() * best.maxH &&
                    limit.maxFps > best.maxFps
                ) {
                    best = limit
                }
            }
            return best
        }

        private fun readLimit(info: MediaCodecInfo, mime: String, name: String): HwLimit? {
            val caps = info.getCapabilitiesForType(mime).videoCapabilities ?: return null
            val maxW = caps.supportedWidths.upper
            val maxH = caps.supportedHeights.upper
            if (maxW < 128 || maxH < 128) return null
            val align = maxOf(caps.widthAlignment, caps.heightAlignment, 2)
            var fps = 30
            try {
                // Probe at the codec's real ceiling and common tablet sizes — not only 1080p.
                val probes = linkedSetOf(
                    maxW to maxH,
                    maxH to maxW,
                    maxW.coerceAtMost(2560) to maxH.coerceAtMost(1600),
                    maxW.coerceAtMost(2560) to maxH.coerceAtMost(1440),
                    maxW.coerceAtMost(1920) to maxH.coerceAtMost(1080),
                )
                for ((pw, ph) in probes) {
                    if (pw < 128 || ph < 128) continue
                    try {
                        if (caps.isSizeSupported(pw, ph)) {
                            fps = maxOf(fps, caps.getSupportedFrameRatesFor(pw, ph).upper.toInt())
                        }
                    } catch (_: Throwable) {
                    }
                }
            } catch (_: Throwable) {
            }
            if (fps < 24) fps = 30
            return HwLimit(name, maxW, maxH, fps, align)
        }

        private fun hasDecoder(mime: String): Boolean {
            val list = MediaCodecList(MediaCodecList.REGULAR_CODECS)
            return list.codecInfos.any { info ->
                !info.isEncoder && info.supportedTypes.any { it.equals(mime, true) }
            }
        }

        fun isSoftwareName(info: MediaCodecInfo, name: String): Boolean {
            try {
                if (info.isSoftwareOnly) return true
            } catch (_: Throwable) {
            }
            val n = name.lowercase()
            return n.contains("google") || n.contains("c2.android") || n.contains("software")
        }

        fun detectSoc(hardware: String, avcName: String?, hevcName: String?): String {
            val extras = ArrayList<String>()
            extras.add(Build.BOARD.orEmpty())
            extras.add(Build.HARDWARE.orEmpty())
            extras.add(Build.PRODUCT.orEmpty())
            if (Build.VERSION.SDK_INT >= 31) {
                extras.add(Build.SOC_MANUFACTURER.orEmpty())
                extras.add(Build.SOC_MODEL.orEmpty())
            }
            val blob = (listOf(hardware, avcName.orEmpty(), hevcName.orEmpty()) + extras)
                .joinToString(" ")
                .lowercase()
            return classifySoc(blob)
        }

        fun classifySoc(blob: String): String {
            return when {
                blob.contains("tensor") || blob.contains("gs101") || blob.contains("gs201") ||
                    blob.contains("zuma") -> "tensor"
                blob.contains("qcom") || blob.contains("qti") || blob.contains("qualcomm") ||
                    blob.contains("bengal") || blob.contains("msm") || blob.contains("sdm") ||
                    blob.contains("lahaina") || blob.contains("kona") || blob.contains("taro") ||
                    blob.contains("kalama") || blob.contains("pineapple") || blob.contains("waipio") ||
                    blob.contains("holi") || blob.contains("yupik") || blob.contains("nairo") ||
                    blob.contains("crow") || blob.contains("parrot") || blob.contains("ravelin") ||
                    blob.contains("sm6225") || blob.contains("sm7") || blob.contains("sm8") -> "qcom"
                blob.contains("mtk") || blob.contains("mediatek") || blob.contains("dimensity") ||
                    blob.contains("kompanio") || blob.contains("helio") ||
                    blob.contains("mt67") || blob.contains("mt68") || blob.contains("mt69") ||
                    blob.contains("mt81") || blob.contains("mt83") -> "mtk"
                blob.contains("exynos") || blob.contains("universal") || blob.contains("s5e") ||
                    blob.contains("erd") -> "exynos"
                blob.contains("hisi") || blob.contains("kirin") || blob.contains("hi36") ||
                    blob.contains("hi62") || blob.contains("hi55") -> "hisi"
                blob.contains("amlogic") || blob.contains("gxl") || blob.contains("g12") -> "amlogic"
                blob.contains("sprd") || blob.contains("unisoc") || blob.contains("ums") -> "unisoc"
                blob.contains("rk") || blob.contains("rockchip") -> "rockchip"
                blob.contains("allwinner") || blob.contains("sun8") || blob.contains("sun5") -> "allwinner"
                else -> "unknown"
            }
        }

        fun decoderRank(name: String, soc: String): Int {
            val n = name.lowercase()
            val vendor = when {
                n.contains("qti") || n.contains("qcom") -> "qcom"
                n.contains("mtk") -> "mtk"
                n.contains("exynos") || n.contains("sec.") || n.contains("samsung") -> "exynos"
                n.contains("hisi") || n.contains("kirin") || n.contains("msvdx") -> "hisi"
                n.contains("amlogic") -> "amlogic"
                n.contains("sprd") || n.contains("unisoc") -> "unisoc"
                n.contains("rk") || n.contains("rockchip") -> "rockchip"
                n.contains("google") && n.contains("c2.") -> "tensor"
                n.contains("c2.") && !n.contains("android") -> "other-hw"
                else -> "other"
            }
            val match = if (vendor == soc) 0 else 10
            val family = when (vendor) {
                "qcom", "mtk", "exynos", "hisi", "tensor" -> 0
                "amlogic", "unisoc", "rockchip" -> 1
                "other-hw" -> 2
                else -> 3
            }
            return match + family
        }
    }
}
