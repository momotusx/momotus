package app.marquee.android

import androidx.compose.runtime.mutableStateMapOf
import app.marquee.core.client.CoreLog
import app.marquee.core.session.CoreSession
import app.marquee.core.views.VersionsView
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch

/** What the ranking needs to know about one video; screens asking for the same share one view. */
data class VersionsKey(
    val kind: String,
    val videoId: String,
    /** The movie or series name, never an episode title (ranking uses it to hide wrong content). */
    val title: String,
    val season: Int? = null,
    val episode: Int? = null,
    /** The release line of the episode that just played, so the next one is picked from the same line. */
    val bingeGroup: String? = null,
)

/**
 * The open `versions` views, shared by the title page (for its play target, so the line under
 * Play is ready), the Versions screen and the player, which keeps the core to one fan-out over
 * the add-ons per video. At most [MAX_OPEN] stay open, the least recently asked for is closed
 * first, so a title page's own view survives a visit to the Versions screen of an episode and
 * Back finds it still full. [closeAll] runs when no screen that needs one is left. Main thread only.
 */
class VersionsHolder(
    private val session: CoreSession,
    private val caps: CapsSource,
    private val scope: CoroutineScope,
) {
    private val views = mutableStateMapOf<VersionsKey, VersionsView>()
    private val opening = HashMap<VersionsKey, Job>()
    private val recent = ArrayList<VersionsKey>()

    /** The open view for [key], or null while the device capabilities are read or before [use]. Read by composables. */
    fun view(key: VersionsKey): VersionsView? = views[key]

    fun use(key: VersionsKey) {
        recent.remove(key)
        recent.add(key)
        if (key !in views && key !in opening) opening[key] = scope.launch { open(key) }
        while (recent.size > MAX_OPEN) close(recent.first())
    }

    private suspend fun open(key: VersionsKey) {
        try {
            val params = caps()
            views[key] =
                session.versions(
                    kind = key.kind,
                    videoId = key.videoId,
                    title = key.title,
                    caps = params,
                    season = key.season,
                    episode = key.episode,
                    bingeGroup = key.bingeGroup,
                )
            opening.remove(key)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            opening.remove(key)
            CoreLog.w("versions could not open: ${e.message}")
        }
    }

    private fun close(key: VersionsKey) {
        recent.remove(key)
        opening.remove(key)?.cancel()
        views.remove(key)?.close()
    }

    fun closeAll() = recent.toList().forEach(::close)

    private companion object {
        const val MAX_OPEN = 2
    }
}
