package app.marquee.macrobenchmark

import androidx.benchmark.macro.MacrobenchmarkScope
import androidx.test.uiautomator.By
import androidx.test.uiautomator.BySelector
import androidx.test.uiautomator.UiObject2
import androidx.test.uiautomator.Until

/** The benchmark builds' own package (app/build.gradle.kts), so a run never touches a device's Marquee. */
const val PACKAGE = "app.marquee.android.benchmark"

/** The profile this emulator walks with: the start picker asks once per cold start when there are two. */
private const val PROFILE = "Main"

/** From a cold start to Home with its rows: past the start picker when it shows. */
fun MacrobenchmarkScope.openHome() {
    device.wait(Until.findObject(By.text(PROFILE)), 5_000)?.click()
    device.wait(Until.hasObject(By.textContains("Popular")), 20_000)
}

/** The title a start opens on (Sintel: free, and Cinemeta has it), as the launch extra names it. */
const val TITLE = "movie:tt1727587"

/** A start straight onto [TITLE]'s page (debug and benchmark builds read the extra), until its details show. */
fun MacrobenchmarkScope.openTitle() {
    startActivityAndWait { it.putExtra("title", TITLE) }
    device.wait(Until.hasObject(By.descStartsWith("IMDb")), 20_000)
}

/**
 * A fresh install opens on onboarding: Start fresh with the recommended addons and Start fast, as a new viewer
 * does, so later starts open on Home. Nothing when Home is already there.
 */
fun MacrobenchmarkScope.onboardIfAsked() {
    val fresh = device.wait(Until.findObject(By.text("Start fresh")), 8_000) ?: return
    fresh.click()
    device.wait(Until.findObject(By.text("Continue")), 10_000)?.click()
    device.wait(Until.findObject(By.text("Start fast")), 10_000)?.click()
    // The subtitles step, with the device's language chosen.
    device.wait(Until.findObject(By.text("Continue")), 10_000)?.click()
    device.wait(Until.hasObject(By.textContains("Popular")), 30_000)
}

/** One thumb swipe over the middle of the screen, [up] towards the top (the list moves down). */
private fun MacrobenchmarkScope.swipe(up: Boolean) {
    val x = device.displayWidth / 2
    val low = device.displayHeight * 3 / 4
    val high = device.displayHeight / 4
    // Coordinates, not a found list: the list recomposes while it moves, which makes a held node stale.
    if (up) device.swipe(x, low, x, high, 12) else device.swipe(x, high, x, low, 12)
    device.waitForIdle()
}

/** Down Home's rows and back up, as a thumb does. */
fun MacrobenchmarkScope.scrollHome() {
    repeat(4) { swipe(up = true) }
    repeat(4) { swipe(up = false) }
}

/** Waits for [selector] near Home's top (Continue Watching sits under the hero), scrolling back up if it moved. */
private fun MacrobenchmarkScope.find(selector: BySelector): UiObject2? {
    repeat(3) {
        device.wait(Until.findObject(selector), 10_000)?.let { return it }
        swipe(up = false)
    }
    return null
}

/**
 * Continue Watching's first title (Breaking Bad, S1:E1 from the local test addon on this emulator), its title
 * page, then Resume or Play, until the player shows. The emulator's media volume is 0 for every run.
 */
fun MacrobenchmarkScope.startPlayback() {
    find(By.desc("Breaking Bad"))?.click() ?: error("Breaking Bad is not in Continue Watching")
    val play =
        device.wait(Until.findObject(By.textStartsWith("Resume")), 10_000)
            ?: device.wait(Until.findObject(By.textStartsWith("Play")), 5_000)
            ?: error("the title page has no Play")
    play.click()
    // The player's own row of actions: it is up and the video is on its way.
    device.wait(Until.hasObject(By.text("Audio")), 20_000)
    device.waitForIdle()
}

/** A start in the TV tree (`form=tv`, read by debug and benchmark builds), on a phone as on a TV, past onboarding. */
fun MacrobenchmarkScope.startTv() {
    startActivityAndWait { it.putExtra("form", "tv") }
    onboardIfAsked()
}

/** [times] D-pad presses [gapMs] apart, then a wait until the screen settles. */
private fun MacrobenchmarkScope.dpad(
    down: Boolean,
    times: Int,
    gapMs: Long,
) {
    repeat(times) {
        if (down) device.pressDPadDown() else device.pressDPadUp()
        Thread.sleep(gapMs)
    }
    device.waitForIdle()
}

/** A viewer stepping through rows: each move lands before the next press. */
private const val STEP_MS = 400L

/** A held arrow key: the system repeats it about every 50 ms, and the TV shell lets one through every 112. */
private const val HELD_MS = 50L

/** TV Home with the D-pad: down through the lanes a row at a time, on down with the key held, and back up held. */
fun MacrobenchmarkScope.scrollTvHome() {
    dpad(down = true, times = 4, gapMs = STEP_MS)
    dpad(down = true, times = 12, gapMs = HELD_MS)
    dpad(down = false, times = 24, gapMs = HELD_MS)
}

/**
 * Discover on TV (a long poster grid), reached as a remote does: Up from Home's first focus into the pill on Home,
 * Right to Discover, select (a search for the word Discover found nothing on the Pixel, 2026-10-22).
 */
fun MacrobenchmarkScope.openTvDiscover() {
    device.pressDPadUp()
    Thread.sleep(STEP_MS)
    device.pressDPadRight()
    Thread.sleep(STEP_MS)
    device.pressDPadCenter()
    // The first page of the grid comes from the network.
    Thread.sleep(4_000)
    device.waitForIdle()
}

/** Discover's grid with the D-pad: down a row at a time, on down held, and back up held. */
fun MacrobenchmarkScope.scrollTvGrid() {
    dpad(down = true, times = 4, gapMs = STEP_MS)
    dpad(down = true, times = 16, gapMs = HELD_MS)
    dpad(down = false, times = 24, gapMs = HELD_MS)
}
