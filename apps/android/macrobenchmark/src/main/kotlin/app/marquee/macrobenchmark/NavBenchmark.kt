package app.marquee.macrobenchmark

import androidx.benchmark.macro.CompilationMode
import androidx.benchmark.macro.FrameTimingMetric
import androidx.benchmark.macro.MacrobenchmarkScope
import androidx.benchmark.macro.junit4.MacrobenchmarkRule
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.uiautomator.By
import androidx.test.uiautomator.Until
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Moving around the app as a viewer does, frames and jank (plans/perf-nav.md, the owner 2026-11-19: navigating is
 * laggy): a title opened from Home and left again, the places of the bar or the tabs, and Settings. The TV run on a
 * phone runs the TV tree on the phone's GPU: label its numbers so.
 */
@RunWith(AndroidJUnit4::class)
class NavBenchmark {
    @get:Rule
    val rule = MacrobenchmarkRule()

    @Test
    fun tvNavigate() =
        rule.measureRepeated(
            packageName = PACKAGE,
            metrics = listOf(FrameTimingMetric()),
            iterations = 5,
            startupMode = null,
            compilationMode = CompilationMode.Partial(),
            setupBlock = {
                killProcess()
                startTv()
                openHome()
            },
        ) { walkTv() }

    @Test
    fun phoneNavigate() =
        rule.measureRepeated(
            packageName = PACKAGE,
            metrics = listOf(FrameTimingMetric()),
            iterations = 5,
            startupMode = null,
            compilationMode = CompilationMode.Partial(),
            setupBlock = {
                killProcess()
                startActivityAndWait { it.putExtra("mute", true) }
                onboardIfAsked()
                openHome()
            },
        ) { walkPhone() }
}

private const val STEP = 400L
private const val REST = 2_000L

private fun MacrobenchmarkScope.keys(
    code: Int,
    times: Int,
    gapMs: Long = STEP,
) = repeat(times) {
    device.pressKeyCode(code)
    Thread.sleep(gapMs)
}

/**
 * Home's first lane, along it, OK on a title, down its page and back up, Back; then up into the bar and across
 * Discover, Library and Search (each shows once focus rests on its tab), back to Home, and Settings from the
 * profile's button with a walk down its list.
 */
private fun MacrobenchmarkScope.walkTv() {
    keys(DOWN, 1)
    keys(RIGHT, 3)
    keys(CENTER, 1, REST)
    keys(DOWN, 3)
    keys(UP, 3)
    keys(BACK, 1, REST)
    keys(RIGHT, 2)
    keys(CENTER, 1, REST)
    keys(BACK, 1, REST)
    keys(UP, 4)
    keys(RIGHT, 3, REST)
    keys(LEFT, 3, REST)
    // The profile's button closes the bar's row: from Home's tab, Right past the places and the tools.
    keys(RIGHT, 7)
    keys(CENTER, 1, REST)
    keys(DOWN, 6)
    keys(BACK, 1, REST)
    device.waitForIdle()
}

/** A poster on Home, its page scrolled down and up, Back; then each tab once and Home again. */
private fun MacrobenchmarkScope.walkPhone() {
    // The first poster under the first Popular shelf's header (openHome waited for it).
    val header = device.wait(Until.findObject(By.textContains("Popular")), 5_000) ?: error("no Popular shelf")
    val at = header.visibleBounds
    device.click(at.left + device.displayWidth / 8, at.bottom + device.displayHeight / 12)
    Thread.sleep(REST)
    val x = device.displayWidth / 2
    repeat(2) { device.swipe(x, device.displayHeight * 3 / 4, x, device.displayHeight / 4, 12) }
    repeat(2) { device.swipe(x, device.displayHeight / 4, x, device.displayHeight * 3 / 4, 12) }
    device.pressBack()
    Thread.sleep(REST)
    visitPhoneTabs()
    device.waitForIdle()
}

private const val UP = 19
private const val DOWN = 20
private const val LEFT = 21
private const val RIGHT = 22
private const val CENTER = 23
private const val BACK = 4
