package app.marquee.android

import androidx.lifecycle.SavedStateHandle
import app.marquee.core.route.Route
import org.junit.Test
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/** Start over starts at the beginning once: a player made again for the same route resumes instead. */
class StartOverOnceTest {
    private val route = Route.Player("movie", "tt1", "tt1", "Sintel", startOver = true)

    @Test
    fun `the first player of a start over route starts at the beginning`() {
        assertTrue(startsOver(route, SavedStateHandle()))
    }

    @Test
    fun `a player restored for the same route resumes`() {
        val saved = SavedStateHandle()
        startsOver(route, saved)
        // What the system hands back after it ended the process: the saved state, not a fresh one.
        val restored = SavedStateHandle(saved.keys().associateWith { saved.get<Any>(it) })
        assertFalse(startsOver(route, restored))
    }

    @Test
    fun `a plain play never starts over`() {
        assertFalse(startsOver(route.copy(startOver = false), SavedStateHandle()))
    }
}
