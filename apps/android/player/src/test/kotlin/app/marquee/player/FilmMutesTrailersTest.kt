package app.marquee.player

import android.os.Looper
import android.os.SystemClock
import androidx.media3.common.SimpleBasePlayer
import app.marquee.design.HeroTrailers
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import kotlin.test.assertFalse

/** The Home trailers' sound goes back to muted when a film starts, so coming back from it is never loud. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class FilmMutesTrailersTest {
    @Test
    fun `a film starting puts the trailers' sound back to muted`() {
        val player =
            object : SimpleBasePlayer(Looper.getMainLooper()) {
                override fun getState(): State = State.Builder().build()
            }
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)
        HeroTrailers.sound = true
        try {
            PlaybackController(player, scope, "Sintel", null, null, SystemClock.elapsedRealtime())
            assertFalse(HeroTrailers.sound, "the trailers' sound outlived the film's start")
        } finally {
            scope.cancel()
            HeroTrailers.sound = false
        }
    }
}
