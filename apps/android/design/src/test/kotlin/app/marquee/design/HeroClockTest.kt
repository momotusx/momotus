package app.marquee.design

import androidx.compose.foundation.interaction.DragInteraction
import androidx.compose.foundation.interaction.MutableInteractionSource
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.yield
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** The hero carousel's clock and the trailer's conditions (plans/appletv-polish.md 4.1, decision 5). */
class HeroClockTest {
    @Test
    fun theDwellPagesOnAndOnePageNeverDoes() {
        val clock = HeroClock(3, dwellMs = 8000)
        assertFalse(clock.tick(4000, running = true))
        assertEquals(0.5f, clock.progress, 0.001f)
        assertTrue(clock.tick(4000, running = true))
        assertEquals(0f, clock.progress, 0.001f)
        val one = HeroClock(1, dwellMs = 8000)
        assertFalse(one.tick(60_000, running = true))
    }

    @Test
    fun aHeldClockDoesNotFillAndAPageByHandStopsIt() {
        val clock = HeroClock(3, dwellMs = 8000)
        clock.tick(2000, running = true)
        assertFalse(clock.tick(60_000, running = false))
        assertEquals(0.25f, clock.progress, 0.001f)
        clock.input()
        assertEquals(0f, clock.progress, 0.001f)
        clock.manual()
        assertFalse(clock.tick(60_000, running = true))
        assertTrue(clock.stopped)
    }

    @Test
    fun theDwellNeverCutsAPlayingTrailer() {
        assertFalse(
            heroClockRuns(
                active = true,
                visible = true,
                reducedMotion = false,
                sliding = false,
                trailerPlaying = true,
                pages = 3,
            ),
        )
        assertTrue(
            heroClockRuns(
                active = true,
                visible = true,
                reducedMotion = false,
                sliding = false,
                trailerPlaying = false,
                pages = 3,
            ),
        )
        // Twenty seconds of frames while a trailer plays: no page change.
        val clock = HeroClock(3, dwellMs = 8000)
        val running =
            heroClockRuns(
                active = true,
                visible = true,
                reducedMotion = false,
                sliding = false,
                trailerPlaying = true,
                pages = 3,
            )
        repeat(1200) { assertFalse(clock.tick(16, running)) }
    }

    @Test
    fun theTwoPagesSlideTogetherEdgeToEdge() {
        // B2's hard edge: at every point of a slide, either way, the page going out ends where the one coming in
        // starts, one page width apart, and at rest the new page is in place.
        for (dir in listOf(1, -1)) {
            for (step in 0..20) {
                val t = step / 20f
                assertEquals(dir.toFloat(), heroPageAt(true, dir, t) - heroPageAt(false, dir, t), 0.0001f)
            }
            assertEquals(dir.toFloat(), heroPageAt(true, dir, 0f), 0.0001f)
            assertEquals(0f, heroPageAt(true, dir, 1f), 0.0001f)
            assertEquals(0f, heroPageAt(false, dir, 0f), 0.0001f)
            assertEquals(-dir.toFloat(), heroPageAt(false, dir, 1f), 0.0001f)
        }
        // The owner's B2 numbers.
        assertEquals(8000L, HeroClock.DWELL_MS.toLong())
        assertEquals(650L, HeroClock.SLIDE_MS.toLong())
        assertEquals(0.85f, Tokens.HERO_PARALLAX, 0.0001f)
    }

    @Test
    fun talkBackOrAKeyPressHoldsTheTimedPages() {
        // Review r1, L4; on TV a key pressed holds the pages for 5 s (the owner, 2026-10-26).
        assertTrue(
            heroClockRuns(
                active = true,
                visible = true,
                reducedMotion = false,
                sliding = false,
                trailerPlaying = false,
                pages = 3,
            ),
        )
        assertFalse(
            heroClockRuns(true, true, false, false, false, 3, screenReader = true),
        )
        assertFalse(heroClockRuns(true, true, false, false, false, 3, held = true))
    }

    @Test
    fun thePhonesTimerAlwaysMovesRightThroughManyLoops() {
        // Review r1, L3: page 8 of 8 goes on to a page on its right that shows the first one.
        val n = 8
        val last = heroPagerPage(n, 7)
        assertEquals(0, (last + 1) % n)
        assertTrue(last + 1 < heroPagerCount(n))
        // A dot picks the nearest page that shows its title, in the loop shown.
        assertEquals(heroPagerPage(n, 2), heroNearestPage(n, heroPagerPage(n, 5), 2))
        // One page: one pager page.
        assertEquals(1, heroPagerCount(1))
        assertEquals(0, heroPagerPage(1, 0))
    }

    @Test
    fun aDragByHandStopsTheTimedPages() =
        runBlocking {
            // Review r2, S3: the phone's swipe, as the pager's interaction source reports it.
            val clock = HeroClock(3, dwellMs = 8000)
            val source = MutableInteractionSource()
            val watching = launch(start = CoroutineStart.UNDISPATCHED) { clock.stopOnDrag(source) }
            source.emit(DragInteraction.Start())
            yield()
            watching.cancel()
            assertTrue(clock.stopped)
            assertFalse(clock.tick(60_000, running = true))
        }

    @Test
    fun aTrailerWritesTheSameValue() {
        val clock = HeroClock(3, dwellMs = 8000)
        clock.trailer(0.4f)
        assertEquals(0.4f, clock.progress, 0.001f)
        clock.trailer(3f)
        assertEquals(1f, clock.progress, 0.001f)
    }

    @Test
    fun aPreviewPlaysOnlyWhenEveryConditionHolds() {
        val ok =
            TrailerSignals(
                enabled = true,
                capable = true,
                visible = true,
                reducedMotion = false,
                metered = false,
                powerSave = false,
                screenReader = false,
                tv = false,
                hasTrailer = true,
            )
        assertTrue(trailerAllowed(ok))
        for (blocked in listOf(
            ok.copy(enabled = false),
            ok.copy(capable = false),
            ok.copy(visible = false),
            ok.copy(reducedMotion = true),
            ok.copy(metered = true),
            ok.copy(powerSave = true),
            ok.copy(screenReader = true),
            ok.copy(tv = true),
            ok.copy(hasTrailer = false),
        )) {
            assertFalse(blocked.toString(), trailerAllowed(blocked))
        }
    }

    @Test
    fun onlyYouTubesIdShapeGoesIntoThePlayer() {
        assertEquals("cdx31ak4KbQ", HeroTrailers.checkedId("cdx31ak4KbQ"))
        assertNull(HeroTrailers.checkedId("cdx31ak4KbQ&list=x"))
        assertNull(HeroTrailers.checkedId("abc'); alert(1); ('"))
        assertNull(HeroTrailers.checkedId(null))
    }
}
