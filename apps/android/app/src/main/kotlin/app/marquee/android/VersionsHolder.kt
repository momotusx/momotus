package app.marquee.android

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import app.marquee.core.client.CoreLog
import app.marquee.core.client.DeviceCapsParams
import app.marquee.core.session.CoreSession
import app.marquee.core.views.VersionsView
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Deferred
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch

/** What the ranking needs to know about one video; two screens asking for the same share one view. */
data class VersionsKey(
    val kind: String,
    val videoId: String,
    /** The movie or series name, never an episode title (ranking uses it to hide wrong content). */
    val title: String,
    val season: Int? = null,
    val episode: Int? = null,
)

/**
 * The one open `versions` view. The title page opens it for its play target so the line under
 * Play is ready, the Versions screen shows the same view, and the player starts from it, which
 * keeps the core to one fan-out over the add-ons at a time. Asking for another video closes the
 * previous view. Main thread only.
 */
class VersionsHolder(
    private val session: CoreSession,
    private val caps: Deferred<DeviceCapsParams>,
    private val scope: CoroutineScope,
) {
    var key: VersionsKey? by mutableStateOf(null)
        private set

    /** The view for [key], null until the device capabilities are known and the view is open. */
    var view: VersionsView? by mutableStateOf(null)
        private set

    private var opening: Job? = null

    fun use(wanted: VersionsKey) {
        if (key == wanted) return
        close()
        key = wanted
        opening =
            scope.launch {
                try {
                    val params = caps.await()
                    view =
                        session.versions(
                            kind = wanted.kind,
                            videoId = wanted.videoId,
                            title = wanted.title,
                            caps = params,
                            season = wanted.season,
                            episode = wanted.episode,
                        )
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    CoreLog.w("versions could not open: ${e.message}")
                }
            }
    }

    fun close() {
        opening?.cancel()
        opening = null
        view?.close()
        view = null
        key = null
    }
}
