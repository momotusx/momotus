package app.marquee.player

import android.os.Handler
import android.os.Looper
import android.os.Message
import androidx.media3.common.C
import androidx.media3.common.Format
import androidx.media3.common.MimeTypes
import androidx.media3.common.text.Cue
import androidx.media3.common.text.CueGroup
import androidx.media3.decoder.DecoderInputBuffer
import androidx.media3.exoplayer.BaseRenderer
import androidx.media3.exoplayer.FormatHolder
import androidx.media3.exoplayer.Renderer
import androidx.media3.exoplayer.RendererCapabilities
import androidx.media3.exoplayer.text.TextOutput
import androidx.media3.extractor.text.CueDecoder
import androidx.media3.extractor.text.CuesWithTiming
import app.marquee.core.client.CoreLog

/**
 * The text renderer for the file's own subtitle tracks, which draws the cues of the position minus the track's
 * [delay] (later when positive), read as it draws: a delay step shows at once and never sets the item again.
 * Media3's own TextRenderer is final, reads cues at most a second ahead and takes no offset, so an earlier delay
 * could only come from parsing the file again. This one reads the track as far as the player has loaded it (the
 * load control keeps up to 50 s ahead) and keeps cues for [SUBTITLE_MAX_MS] behind, so a delay either way finds its
 * cues. After a seek nothing before the new position was read: a later delay shows its first cues that much later.
 *
 * It takes the cues the extractors parsed (Media3's own sample format, the default for every source since 1.4);
 * captions carried in the video (CEA-608 and 708) stay with Media3's TextRenderer, without a delay.
 */
class DelayedTextRenderer(
    private val output: TextOutput,
    looper: Looper,
    private val delay: SubtitleDelay,
) : BaseRenderer(C.TRACK_TYPE_TEXT),
    Handler.Callback {
    private val handler = Handler(looper, this)
    private val formatHolder = FormatHolder()
    private val buffer = DecoderInputBuffer(DecoderInputBuffer.BUFFER_REPLACEMENT_MODE_NORMAL)
    private val decoder = CueDecoder()
    private val cues = TimedCues()
    private var inputEnded = false
    private var shown: List<Cue> = emptyList()

    /** Where the last render was, for a redraw between renders ([MSG_REDRAW]); unset while disabled. */
    private var lastPositionUs = C.TIME_UNSET

    override fun getName(): String = "DelayedTextRenderer"

    override fun supportsFormat(format: Format): Int =
        when {
            format.sampleMimeType == MimeTypes.APPLICATION_MEDIA3_CUES -> {
                RendererCapabilities.create(
                    if (format.cryptoType == C.CRYPTO_TYPE_NONE) C.FORMAT_HANDLED else C.FORMAT_UNSUPPORTED_DRM,
                )
            }

            MimeTypes.isText(format.sampleMimeType) -> {
                RendererCapabilities.create(C.FORMAT_UNSUPPORTED_SUBTYPE)
            }

            else -> {
                RendererCapabilities.create(C.FORMAT_UNSUPPORTED_TYPE)
            }
        }

    override fun render(
        positionUs: Long,
        elapsedRealtimeUs: Long,
    ) {
        lastPositionUs = positionUs
        draw(positionUs)
    }

    /** A delay change, sent through the player (PlayerHost): drawn now, not at the next render. */
    override fun handleMessage(
        messageType: Int,
        message: Any?,
    ) {
        if (messageType == MSG_REDRAW &&
            lastPositionUs != C.TIME_UNSET
        ) {
            draw(lastPositionUs)
        } else {
            super.handleMessage(messageType, message)
        }
    }

    private fun draw(positionUs: Long) {
        val at = positionUs - delay.ms * 1000
        readAhead(at)
        cues.discardBefore(positionUs - SUBTITLE_MAX_MS * 1000)
        cues.trimPictures(at, MAX_PICTURES_BEHIND)
        val now = cues.at(at)
        if (now != shown) {
            shown = now
            handler.obtainMessage(0, CueGroup(now, at)).sendToTarget()
        }
    }

    /**
     * Every cue the player has loaded so far; a bounded batch a call, so a long file never holds a frame up. Picture
     * cues (PGS, DVB) come decoded, a few MB each at full frame: past [MAX_PICTURES_AHEAD] of them ahead of [at], or
     * [MAX_PICTURE_BYTES] held, the rest wait in the player's buffer.
     */
    private fun readAhead(at: Long) {
        repeat(READ_BATCH) {
            if (inputEnded) return
            if (cues.picturesFrom(at) >= MAX_PICTURES_AHEAD || cues.pictureBytes() >= MAX_PICTURE_BYTES) return
            buffer.clear()
            when (readSource(formatHolder, buffer, 0)) {
                C.RESULT_BUFFER_READ -> {
                    if (buffer.isEndOfStream) {
                        inputEnded = true
                        return
                    }
                    buffer.flip()
                    val data = buffer.data ?: return@repeat
                    // A cue that does not decode is skipped, as Media3's own renderer does; the video goes on.
                    try {
                        cues.add(decoder.decode(buffer.timeUs, data.array(), data.arrayOffset(), data.limit()))
                    } catch (e: RuntimeException) {
                        CoreLog.w("a subtitle cue at ${buffer.timeUs} us did not decode: ${e.message}")
                    }
                }

                C.RESULT_FORMAT_READ -> {
                    Unit
                }

                else -> {
                    return
                }
            }
        }
    }

    override fun onPositionReset(
        positionUs: Long,
        joining: Boolean,
        sampleStreamIsResetToKeyFrame: Boolean,
    ) {
        cues.clear()
        inputEnded = false
        clear()
    }

    override fun onDisabled() {
        lastPositionUs = C.TIME_UNSET
        cues.clear()
        clear()
    }

    private fun clear() {
        if (shown.isEmpty()) return
        shown = emptyList()
        handler.obtainMessage(0, CueGroup(emptyList(), C.TIME_UNSET)).sendToTarget()
    }

    // Subtitles never hold playback up; the video ends the play.
    override fun isReady(): Boolean = true

    override fun isEnded(): Boolean = inputEnded

    override fun handleMessage(msg: Message): Boolean {
        output.onCues(msg.obj as CueGroup)
        return true
    }

    companion object {
        /** The player message that redraws at the last position: a delay changed. */
        const val MSG_REDRAW = Renderer.MSG_CUSTOM_BASE + 1

        private const val READ_BATCH = 64

        /** Picture cues held ahead of what shows and behind it, and their bytes at most. */
        internal const val MAX_PICTURES_AHEAD = 16
        internal const val MAX_PICTURES_BEHIND = 8
        internal const val MAX_PICTURE_BYTES = 64L * 1024 * 1024
    }
}

/**
 * Cues with their times, as a subtitle track gives them: what shows at a moment, overlapping cues together. A cue
 * with no length (a file whose cues replace each other) lasts until the next one starts.
 */
class TimedCues {
    private val all = ArrayList<CuesWithTiming>()

    fun add(cues: CuesWithTiming) {
        // Usually in order; a cue out of order goes to its place.
        var i = all.size
        while (i > 0 && all[i - 1].startTimeUs > cues.startTimeUs) i--
        all.add(i, cues)
    }

    fun addAll(cues: List<CuesWithTiming>) = cues.forEach(::add)

    private fun endOf(i: Int): Long {
        val c = all[i]
        return if (c.durationUs !=
            C.TIME_UNSET
        ) {
            c.startTimeUs + c.durationUs
        } else {
            all.getOrNull(i + 1)?.startTimeUs ?: Long.MAX_VALUE
        }
    }

    /**
     * What shows at [timeUs]. ponytail: a scan of every cue kept, a few thousand comparisons; a search by start if a
     * track ever holds far more.
     */
    fun at(timeUs: Long): List<Cue> =
        all.indices.filter { all[it].startTimeUs <= timeUs && timeUs < endOf(it) }.flatMap { all[it].cues }

    private fun picture(c: CuesWithTiming) = c.cues.any { it.bitmap != null }

    /** The picture cues starting at or after [timeUs]. */
    fun picturesFrom(timeUs: Long): Int = all.count { picture(it) && it.startTimeUs >= timeUs }

    /** The bytes the picture cues hold. */
    fun pictureBytes(): Long = all.sumOf { c -> c.cues.sumOf { it.bitmap?.allocationByteCount?.toLong() ?: 0L } }

    /** Keeps the last [keep] picture cues that ended by [timeUs], the newest; text cues stay. */
    fun trimPictures(
        timeUs: Long,
        keep: Int,
    ) {
        val behind = all.indices.filter { picture(all[it]) && endOf(it) <= timeUs }
        if (behind.size <= keep) return
        val gone = behind.dropLast(keep).map { all[it] }.toSet()
        all.removeAll(gone)
    }

    /** Forgets the cues that ended before [timeUs]. */
    fun discardBefore(timeUs: Long) {
        val keep = all.indices.filter { endOf(it) >= timeUs }.map { all[it] }
        if (keep.size == all.size) return
        all.clear()
        all.addAll(keep)
    }

    fun clear() = all.clear()
}
