package app.marquee.player

import androidx.media3.common.C
import androidx.media3.common.Format
import androidx.media3.common.text.Cue
import androidx.media3.extractor.text.CuesWithTiming
import androidx.media3.extractor.text.DefaultSubtitleParserFactory
import androidx.media3.extractor.text.SubtitleParser
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.net.HttpURLConnection
import java.net.URL

/**
 * An addon subtitle's cues, parsed once from its file: the player draws them over the picture for the position, so a
 * pick, Off or a delay step never sets the media item again (as the Apple app draws its own).
 */
class AddonCues(
    cues: List<CuesWithTiming>,
) {
    private val all = cues.sortedBy { it.startTimeUs }

    // A cue with no length (a file whose cues replace each other) lasts until the next one starts.
    private val ends =
        all.mapIndexed { i, c ->
            val next = all.getOrNull(i + 1)?.startTimeUs ?: Long.MAX_VALUE
            if (c.durationUs == C.TIME_UNSET) next else c.startTimeUs + c.durationUs
        }

    /**
     * What shows at [positionUs] of the file's own time. ponytail: a scan of every cue a frame, a few thousand
     * comparisons; a search by start if a file ever holds far more.
     */
    fun at(positionUs: Long): List<Cue> =
        all.indices.filter { all[it].startTimeUs <= positionUs && positionUs < ends[it] }.flatMap { all[it].cues }
}

/** Parses an addon subtitle file with Media3's own parsers (WebVTT from the core's loopback, SubRip, SSA, TTML). */
fun parseAddonCues(
    bytes: ByteArray,
    mimeType: String,
): AddonCues {
    val parser = DefaultSubtitleParserFactory().create(Format.Builder().setSampleMimeType(mimeType).build())
    val cues = ArrayList<CuesWithTiming>()
    parser.parse(bytes, SubtitleParser.OutputOptions.allCues()) { cues += it }
    return AddonCues(cues)
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
