package app.marquee.player

import app.marquee.core.model.SubtitleSize
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/** The subtitle delay's steps and words, and the profile's subtitle size (journey reviews 2026-10-20). */
class SubtitleTimingTest {
    @Test
    fun `the Timing row says where the words are`() {
        assertEquals("0.0 seconds", timingWords(0))
        assertEquals("Plus 0.5 seconds", timingWords(500))
        assertEquals("Minus 1.5 seconds", timingWords(-1_500))
        assertEquals("0.0 s", delayText(0))
        assertEquals("+0.3 s", delayText(300))
        assertEquals("\u22121.2 s", delayText(-1_200))
        // At least one decimal, at most two, a true minus sign (D3).
        assertEquals("+0.25 s", delayText(250))
        assertEquals("+0.5 s", delayText(500))
        assertEquals("+1.0 s", delayText(1_000))
        assertEquals("\u22120.25 s", delayText(-250))
        assertEquals("+0.75 s", delayText(750))
        assertEquals("Plus 0.25 seconds", timingWords(250))
    }

    @Test
    fun `a quick shift tap moves 0_25 s, four make 1_0 s, within a minute, on the 0_05 s grid`() {
        assertEquals(250L, QUICK_STEP_MS)
        assertEquals(50L, DELAY_GRID_MS)
        val four = (1..4).fold(0L) { at, _ -> quickDelay(at, 1) }
        assertEquals("+1.0 s", delayText(four))
        assertEquals("\u22120.25 s", delayText(quickDelay(0, -1)))
        assertEquals(60_000L, quickDelay(59_900, 1))
        assertEquals(-60_000L, quickDelay(-59_900, -1, heldMs = 3_000))
        // One set at 0.1 s is on the grid already; one off it lands on it at the first tap, short of the full step.
        assertEquals(550L, quickDelay(300, 1))
        assertEquals(50L, quickDelay(300, -1))
        assertEquals(450L, quickDelay(237, 1))
        assertEquals(0L, quickDelay(237, -1))
    }

    @Test
    fun `held, the quick shift ticks from 400 ms every 150 ms, growing to four steps, so 2 s moves 4_5 s`() {
        assertEquals(400L, QUICK_START_MS)
        assertEquals(150L, QUICK_EVERY_MS)
        assertEquals(250L, quickDelay(0, 1, heldMs = 400))
        assertEquals(500L, quickDelay(0, 1, heldMs = 1_000))
        assertEquals(750L, quickDelay(0, 1, heldMs = 2_000))
        assertEquals(1_000L, quickDelay(0, 1, heldMs = 3_000))
        assertEquals(1_000L, quickDelay(0, 1, heldMs = 30_000))

        // The ticks a hold of [ms] gives, as DelayHold runs them, from 0.
        fun held(ms: Long) =
            generateSequence(QUICK_START_MS) { it + QUICK_EVERY_MS }
                .takeWhile { it < ms }
                .fold(0L) { at, tick -> quickDelay(at, 1, tick) }
        assertEquals(4_500L, held(2_000))
        // The remote's pinned case (section 6): let go at 990 ms, ticks at 400, 550, 700 and 850.
        assertEquals(1_000L, held(990))
    }

    @Test
    fun `the shift is said once settled, and its value is named with what a press does`() {
        assertEquals("Subtitles 0.25 seconds later", shiftSaid(250))
        assertEquals("Subtitles 1.0 seconds earlier", shiftSaid(-1_000))
        assertEquals("Subtitles in sync", shiftSaid(0))
        assertEquals("+0.25 s, subtitles later, press to reset", shiftName(250, tv = false))
        assertEquals("\u22120.5 s, subtitles earlier, press to reset", shiftName(-500, tv = false))
        // At 0 a press does nothing, so it is not offered; a remote's OK always adjusts.
        assertEquals("0.0 s, subtitles in sync", shiftName(0, tv = false))
        assertEquals("0.0 s, subtitles in sync, press OK to adjust", shiftName(0, tv = true))
    }

    @Test
    fun `a press steps a tenth of a second and snaps to one, a hold steps 0_3 s rising to 0_6 s, within a minute`() {
        assertEquals(100L, SUBTITLE_STEP_MS)
        assertEquals(100L, steppedDelay(0, 1))
        assertEquals(-100L, steppedDelay(0, -1))
        // Every delay sits on the 0.05 s grid, so the quick shift's 0.25 s and these 0.1 s steps meet (D3); a delay off
        // it lands on it at the first press, short of the full step.
        assertEquals(350L, steppedDelay(250, 1))
        assertEquals(150L, steppedDelay(250, -1))
        assertEquals(-350L, steppedDelay(-250, -1))
        assertEquals(300L, steppedDelay(237, 1))
        assertEquals(150L, steppedDelay(237, -1))
        // Held: the interval starts after 250 ms, so the first repeat is at 350, then every 100 ms; 0.3 s a tick in
        // the first second, then 0.4, 0.5, 0.6.
        assertEquals(250L, HOLD_START_MS)
        assertEquals(100L, HOLD_EVERY_MS)
        assertEquals(300L, steppedDelay(0, 1, heldMs = 250))
        assertEquals(300L, steppedDelay(0, 1, heldMs = 950))
        assertEquals(400L, steppedDelay(0, 1, heldMs = 1_050))
        assertEquals(500L, steppedDelay(0, 1, heldMs = 2_050))
        assertEquals(-600L, steppedDelay(0, -1, heldMs = 3_050))
        assertEquals(600L, steppedDelay(0, 1, heldMs = 60_000))
        // Plus or minus a minute at most.
        assertEquals(60_000L, SUBTITLE_MAX_MS)
        assertEquals(60_000L, steppedDelay(59_900, 1, heldMs = 5_000))
        assertEquals(-60_000L, steppedDelay(-60_000, -1))
        // Held from 0, a minute is 109 ticks away: 11 s of holding.
        val ticks =
            generateSequence(HOLD_START_MS + HOLD_EVERY_MS) { it + HOLD_EVERY_MS }
                .runningFold(0L) { at, held -> steppedDelay(at, 1, held) }
                .indexOfFirst { it == SUBTITLE_MAX_MS }
        assertEquals(109, ticks)
        assertTrue(generateSequence(1L) { it + 37 }.take(200).all { steppedDelay(it, 1) % DELAY_GRID_MS == 0L })
    }

    @Test
    fun `the profile's size scales the system caption size`() {
        assertEquals(1f, subtitleScale(SubtitleSize.Medium))
        assertEquals(0.75f, subtitleScale(SubtitleSize.Small))
        assertEquals(1.3f, subtitleScale(SubtitleSize.Large))
        assertEquals(1.6f, subtitleScale(SubtitleSize.ExtraLarge))
        assertEquals(1f, subtitleScale(SubtitleSize.Unknown))
    }
}
