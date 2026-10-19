package app.marquee.macrobenchmark

import androidx.benchmark.macro.MacrobenchmarkScope
import androidx.test.uiautomator.By
import androidx.test.uiautomator.BySelector
import androidx.test.uiautomator.UiObject2
import androidx.test.uiautomator.Until

const val PACKAGE = "app.marquee.android"

/** The profile this emulator walks with: the start picker asks once per cold start when there are two. */
private const val PROFILE = "Main"

/** From a cold start to Home with its rows: past the start picker when it shows. */
fun MacrobenchmarkScope.openHome() {
    device.wait(Until.findObject(By.text(PROFILE)), 5_000)?.click()
    device.wait(Until.hasObject(By.textContains("Popular")), 20_000)
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
