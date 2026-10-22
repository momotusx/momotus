package app.marquee.player

import androidx.media3.common.Format
import androidx.media3.extractor.text.CuesWithTiming
import androidx.media3.extractor.text.DefaultSubtitleParserFactory
import androidx.media3.extractor.text.SubtitleParser
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.net.HttpURLConnection
import java.net.URL

/** Parses an addon subtitle file with Media3's own parsers (WebVTT from the core's loopback, SubRip, SSA, TTML). */
fun parseAddonCues(
    bytes: ByteArray,
    mimeType: String,
): TimedCues {
    val parser = DefaultSubtitleParserFactory().create(Format.Builder().setSampleMimeType(mimeType).build())
    val cues = ArrayList<CuesWithTiming>()
    parser.parse(bytes, SubtitleParser.OutputOptions.allCues()) { cues += it }
    return TimedCues().apply { addAll(cues) }
}

/** An addon subtitle file from the core's loopback server; an HTTP error or a dropped connection throws. */
suspend fun fetchSubtitle(url: String): ByteArray =
    withContext(Dispatchers.IO) {
        val connection = URL(url).openConnection() as HttpURLConnection
        connection.connectTimeout = FETCH_TIMEOUT_MS
        connection.readTimeout = FETCH_TIMEOUT_MS
        try {
            connection.inputStream.use { it.readBytes() }
        } finally {
            connection.disconnect()
        }
    }

/** The core fetches the addon's file before it answers: its own limits run first, this only ends a hang. */
private const val FETCH_TIMEOUT_MS = 30_000
