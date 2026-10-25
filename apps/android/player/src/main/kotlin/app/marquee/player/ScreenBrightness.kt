package app.marquee.player

import android.content.ContentResolver
import android.provider.Settings
import android.view.Window
import android.view.WindowManager
import kotlin.math.pow

/**
 * The player window's brightness as the phone's left-hand swipe sets it (plans/player-gestures.md 3.3, 3.5): the
 * window's own value only, never the system setting. The level is held here, not read back from the window, so the
 * floor of [brightnessToWindow] never turns into a higher start; one instance serves every player, so the next
 * episode starts from the viewer's level and only the last player out lets the system's back in.
 */
class ScreenBrightness {
    /** The level last swiped, 0 to 1; null while the window follows the system. */
    var level: Float? = null
        private set

    /** Where a swipe starts: the held level, else the system's, read as a level. */
    fun start(resolver: ContentResolver): Float =
        startLevel(level, Settings.System.getInt(resolver, Settings.System.SCREEN_BRIGHTNESS, 128))

    fun set(
        window: Window,
        to: Float,
    ) {
        level = to
        apply(window)
    }

    /** Puts the held level on the window again (back from picture in picture); nothing when none is held. */
    fun apply(window: Window) {
        val held = level ?: return
        window.attributes = window.attributes.apply { screenBrightness = brightnessToWindow(held) }
    }

    /** The window follows the system again; [forget] also drops the level (the player left, or the system's changed). */
    fun release(
        window: Window,
        forget: Boolean,
    ) {
        if (forget) level = null
        // No layout pass when nothing was swiped.
        if (window.attributes.screenBrightness == WindowManager.LayoutParams.BRIGHTNESS_OVERRIDE_NONE) return
        window.attributes =
            window.attributes.apply { screenBrightness = WindowManager.LayoutParams.BRIGHTNESS_OVERRIDE_NONE }
    }
}

// ponytail: a 2.2 power curve stands in for the system's; its own if it is ever public.

/**
 * A level (0 to 1, as the viewer sees it) as the window's value. Android's own slider is perceptual while the
 * window's value is linear, and its curve (BrightnessUtils) is hidden. The 0.01 floor is VLC's: some panels go off at 0.
 */
fun brightnessToWindow(level: Float): Float = level.coerceIn(0f, 1f).pow(GAMMA).coerceAtLeast(0.01f)

fun windowToLevel(value: Float): Float = value.coerceIn(0f, 1f).pow(1f / GAMMA)

/** The held level when there is one, else the system setting (0 to 255) as a level. */
fun startLevel(
    held: Float?,
    setting: Int,
): Float = held ?: windowToLevel(setting / 255f)

private const val GAMMA = 2.2f
