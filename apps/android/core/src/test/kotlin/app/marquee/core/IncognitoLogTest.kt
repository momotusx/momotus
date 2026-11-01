package app.marquee.core

import app.marquee.core.client.CoreLog
import java.io.IOException
import kotlin.test.AfterTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** Incognito's log lines name no title, address or stream: logcat is a file on the device (plans/batch-10-incognito.md). */
class IncognitoLogTest {
    private val lines = mutableListOf<Triple<Char, String, Throwable?>>()
    private val sink = CoreLog.sink
    private val quiet = CoreLog.quiet

    init {
        CoreLog.sink = { level, message, error -> lines += Triple(level, message, error) }
    }

    @AfterTest
    fun restore() {
        CoreLog.sink = sink
        CoreLog.quiet = quiet
    }

    // What a Media3 playback error and a subtitle failure say, as PlaybackController logs them.
    private val stream = "http://127.0.0.1:41231/stream/0123456789abcdef0123456789abcdef01234567/1"
    private val failure = IOException("Response code: 404 for $stream")

    private fun logAll() {
        CoreLog.d("progress for tt0013442")
        CoreLog.w("playback error ERROR_CODE_IO_BAD_HTTP_STATUS: ${failure.message}", failure)
        CoreLog.e("subtitle file https://subs.example/tt0013442/en.srt: kitsu:1376:1:2 and tmdb:653")
    }

    @Test
    fun `in Incognito debug lines go, and the rest keep no id, address, hash or error message`() {
        CoreLog.quiet = { true }
        logAll()
        assertEquals(listOf('w', 'e'), lines.map { it.first })
        val said = lines.joinToString("\n") { it.second }
        for (leak in listOf(
            "tt0013442",
            "127.0.0.1",
            "/stream/",
            "0123456789abcdef",
            "subs.example",
            "kitsu:",
            "tmdb:",
        )) {
            assertFalse(leak in said, "logged \"$leak\": $said")
        }
        assertTrue("(IOException)" in lines[0].second, lines[0].second)
        assertNull(lines[0].third, "the error itself would print its message and trace")
        assertTrue("a title" in lines[1].second, lines[1].second)
    }

    @Test
    fun `outside Incognito every line goes as written`() {
        logAll()
        assertEquals(listOf('d', 'w', 'e'), lines.map { it.first })
        assertTrue("tt0013442" in lines[0].second)
        assertEquals(failure, lines[1].third)
    }
}
