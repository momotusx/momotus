package app.marquee.player

import android.os.Looper
import android.os.SystemClock
import androidx.media3.common.C
import androidx.media3.common.Format
import androidx.media3.common.MediaItem
import androidx.media3.common.MimeTypes
import androidx.media3.common.SimpleBasePlayer
import androidx.media3.common.TrackGroup
import androidx.media3.common.Tracks
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** A zap play (plans/zap.md 2.3): its name hidden until What is this?, and what counts as one that did not play. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class ZapModeTest {
    private fun controller(
        scope: CoroutineScope,
        revealed: Boolean,
    ): PlaybackController {
        val player =
            object : SimpleBasePlayer(Looper.getMainLooper()) {
                override fun getState(): State = State.Builder().build()
            }
        return PlaybackController(
            player,
            scope,
            "Severance",
            "S2:E5",
            null,
            SystemClock.elapsedRealtime(),
            revealed = revealed,
        ).also {
            it.backdrop = "https://img/back.jpg"
            it.logo = "https://img/logo.png"
        }
    }

    @Test
    fun `a zap names nothing until it is revealed, then everything`() {
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)
        try {
            val zap = controller(scope, revealed = false)
            assertEquals("Zap", zap.title)
            assertNull(zap.episode)
            assertNull(zap.backdrop)
            assertNull(zap.logo)
            zap.revealed = true
            assertEquals("Severance", zap.title)
            assertEquals("S2:E5", zap.episode)
            assertEquals("https://img/back.jpg", zap.backdrop)
            assertEquals("https://img/logo.png", zap.logo)
            // An ordinary play names it from the start.
            assertEquals("Severance", controller(scope, revealed = true).title)
        } finally {
            scope.cancel()
        }
    }

    @Test
    fun `a zap that failed before its first frame did not play, one still starting at the offer did`() {
        assertTrue(zapMissed(failed = true, firstFrame = false))
        // Still starting when the core offers another version: it waits for the core (Pixel walk 2026-11-18).
        assertFalse(zapMissed(failed = false, firstFrame = false))
        // After the first frame nothing zaps by itself: a stall offers the bar's Zap again, a failure its card.
        assertFalse(zapMissed(failed = true, firstFrame = true))
    }

    /** One track group of [format]. */
    private fun group(format: Format) =
        Tracks.Group(TrackGroup(format), false, intArrayOf(C.FORMAT_HANDLED), booleanArrayOf(false))

    @Test
    fun `a file's own track labels, often the release name, stay hidden until the zap is revealed`() {
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)
        try {
            val release = "Severance.S02E05.1080p.WEB"
            val audio =
                Format
                    .Builder()
                    .setSampleMimeType(MimeTypes.AUDIO_AAC)
                    .setLabel(release)
                    .build()
            val english =
                Format
                    .Builder()
                    .setSampleMimeType(MimeTypes.TEXT_VTT)
                    .setLanguage("en")
                    .setLabel(release)
                    .build()
            val untagged =
                Format
                    .Builder()
                    .setSampleMimeType(MimeTypes.TEXT_VTT)
                    .setLabel(release)
                    .build()
            var state = SimpleBasePlayer.State.Builder().build()
            val player =
                object : SimpleBasePlayer(Looper.getMainLooper()) {
                    override fun getState(): State = state

                    fun media(tracks: Tracks) {
                        state =
                            state
                                .buildUpon()
                                .setPlaylist(
                                    listOf(
                                        MediaItemData
                                            .Builder("item")
                                            .setMediaItem(MediaItem.fromUri("http://127.0.0.1/v.mkv"))
                                            .setTracks(tracks)
                                            .build(),
                                    ),
                                ).build()
                        invalidateState()
                    }
                }
            val zap =
                PlaybackController(
                    player,
                    scope,
                    "Severance",
                    "S2:E5",
                    null,
                    SystemClock.elapsedRealtime(),
                    revealed = false,
                )
            player.media(Tracks(listOf(group(audio), group(english), group(untagged))))
            shadowOf(Looper.getMainLooper()).idle()
            val shown = { (zap.audio + zap.text).flatMap { listOf(it.name, it.detail) }.joinToString(" | ") }
            assertEquals(listOf("Audio"), zap.audio.map { it.name })
            assertTrue(release !in shown(), shown())
            assertTrue(zap.text.any { it.name == "English" } && zap.text.any { it.name == "Subtitles" }, shown())
            zap.revealed = true
            assertEquals(listOf(release), zap.audio.map { it.name })
            assertTrue(zap.text.any { it.name == release }, shown())
        } finally {
            scope.cancel()
        }
    }

    @Test
    fun `fast dial presses zap once, with the last step`() {
        val scope = CoroutineScope(Dispatchers.Unconfined)
        val writes = ArrayList<CompletableDeferred<Unit>>()
        val zapped = ArrayList<String>()
        var job: Job? = null
        for (step in listOf("Near", "Far")) {
            job =
                scope.dialPress(
                    job,
                    write = { CompletableDeferred<Unit>().also(writes::add).await() },
                    failed = { zapped += "failed" },
                    then = { zapped += step },
                )
        }
        // The first write answers after the second press: it zaps nothing, and is no failure.
        writes[0].complete(Unit)
        writes[1].complete(Unit)
        assertEquals(listOf("Far"), zapped)
        // A write that fails zaps nothing.
        scope.dialPress(
            job,
            write = { error("the core refused") },
            failed = { zapped += "failed" },
            then = { zapped += "Same" },
        )
        assertEquals(listOf("Far", "failed"), zapped)
    }
}
