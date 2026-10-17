package app.marquee.ui.tv

enum class Axis { Horizontal, Vertical }

/**
 * Keeps a held arrow key from queuing more focus moves than the screen can draw: the first press
 * always goes through; while the key repeats, a press passes only when [horizontalMs] (or
 * [verticalMs]) have gone by since the last one that did. A pure function of timestamps, so it is
 * tested without a device.
 */
class DpadThrottle(
    private val horizontalMs: Long = 80,
    private val verticalMs: Long = 112,
) {
    private var lastHorizontal = NEVER
    private var lastVertical = NEVER

    fun allow(
        axis: Axis,
        repeat: Boolean,
        nowMs: Long,
    ): Boolean {
        val last = if (axis == Axis.Horizontal) lastHorizontal else lastVertical
        val gap = if (axis == Axis.Horizontal) horizontalMs else verticalMs
        if (repeat && nowMs - last < gap) return false
        if (axis == Axis.Horizontal) lastHorizontal = nowMs else lastVertical = nowMs
        return true
    }

    private companion object {
        const val NEVER = Long.MIN_VALUE / 2
    }
}
