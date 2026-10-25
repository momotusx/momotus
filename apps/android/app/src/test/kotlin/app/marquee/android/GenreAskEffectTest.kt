package app.marquee.android

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.junit4.createComposeRule
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import kotlin.test.assertEquals
import kotlin.test.assertNull

/**
 * Discover's side of a title page's genre chip (plans/title-page-ratings-genres-similar.md 2): no select before its
 * first snapshot, one select once it is in, the ask cleared after; an ask that never went is dropped when Discover
 * really leaves, and kept through a configuration change.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class GenreAskEffectTest {
    @get:Rule
    val compose = createComposeRule()

    private val drama = DiscoverRequest("movie", "Drama")

    @Test
    fun `one select once Discover has its first snapshot, then the ask is cleared`() {
        var request: DiscoverRequest? by mutableStateOf(drama)
        var ready by mutableStateOf(false)
        val selects = mutableListOf<DiscoverRequest>()
        compose.setContent { GenreAskEffect(request, ready, { selects += it }, { request = null }, { false }) }
        compose.waitForIdle()
        assertEquals(emptyList(), selects, "no select before the first snapshot")
        ready = true
        compose.waitForIdle()
        assertEquals(listOf(drama), selects)
        assertNull(request)
        // Later snapshots send nothing again.
        ready = false
        compose.waitForIdle()
        ready = true
        compose.waitForIdle()
        assertEquals(listOf(drama), selects)
    }

    /** Shows the effect with an ask that cannot go yet, then takes it away; what is left of the ask. */
    private fun leave(changing: Boolean): DiscoverRequest? {
        var request: DiscoverRequest? = drama
        var shown by mutableStateOf(true)
        compose.setContent {
            if (shown) GenreAskEffect(request, ready = false, select = {}, onSent = { request = null }, { changing })
        }
        compose.waitForIdle()
        shown = false
        compose.waitForIdle()
        return request
    }

    @Test
    fun `an ask that never went is dropped when Discover leaves`() {
        assertNull(leave(changing = false))
    }

    @Test
    fun `a configuration change keeps the ask for the Discover that comes back`() {
        assertEquals(drama, leave(changing = true))
    }
}
