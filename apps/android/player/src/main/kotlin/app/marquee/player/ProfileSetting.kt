package app.marquee.player

import app.marquee.core.client.CoreLog
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch

/**
 * A profile setting the player's picker changes (the subtitle size, the two lines' layout, Only when paused): a pick
 * shows at once and is written ([write] answers the profile's value after it). While writes are out the profile's own
 * answers do not move what shows, so fast presses never flicker back; when the last one ends, the profile's value
 * shows, which puts a failed write back.
 */
class ProfileSetting<T>(
    private val scope: CoroutineScope,
    initial: T,
    private val write: suspend (T) -> T,
    private val show: (T) -> Unit,
) {
    private var profile = initial
    private var writing = 0

    /** The profile's value, as its settings view says it. */
    fun fromProfile(value: T) {
        profile = value
        if (writing == 0) show(value)
    }

    /** The viewer's pick from the picker. */
    fun pick(value: T) {
        show(value)
        writing++
        scope.launch {
            try {
                profile = write(value)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                CoreLog.w("a subtitle setting was not kept: ${e.message}")
            } finally {
                writing--
                if (writing == 0) show(profile)
            }
        }
    }
}
