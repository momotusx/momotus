package app.marquee.player

import app.marquee.core.client.CoreLog
import app.marquee.core.model.SubtitleSize
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch

/**
 * The picker's Size against the profile's setting: a pick shows at once and is written ([write] answers the
 * profile's size after it). While writes are out the profile's own answers do not move what shows, so fast presses
 * never flicker back; when the last one ends, the profile's size shows, which puts a failed write back.
 */
class SizeSetting(
    private val scope: CoroutineScope,
    private val write: suspend (SubtitleSize) -> SubtitleSize,
    private val show: (SubtitleSize) -> Unit,
) {
    private var profile = SubtitleSize.Medium
    private var writing = 0

    /** The profile's size, as its settings view says it. */
    fun fromProfile(size: SubtitleSize) {
        profile = size
        if (writing == 0) show(size)
    }

    /** The viewer's pick from the picker. */
    fun pick(size: SubtitleSize) {
        show(size)
        writing++
        scope.launch {
            try {
                profile = write(size)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                CoreLog.w("the subtitle size was not kept: ${e.message}")
            } finally {
                writing--
                if (writing == 0) show(profile)
            }
        }
    }
}
