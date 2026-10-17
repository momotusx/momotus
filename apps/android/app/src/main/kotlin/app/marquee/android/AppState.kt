package app.marquee.android

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import app.marquee.core.client.DeviceCapsParams
import app.marquee.core.session.CoreHealth
import app.marquee.core.session.CoreSession
import app.marquee.core.views.HomeView
import app.marquee.core.views.ViewStatus
import app.marquee.ui.tv.TvHomeState
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Deferred
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch

/** Where the app starts: a new user installs a catalog first, everyone else lands on Home. */
enum class Start { Loading, Onboarding, Home }

/**
 * What lives as long as the activity: the start decision, and Home, which stays open under the
 * pages above it so Back finds its rows, scroll positions and focus exactly as they were. The
 * core's view is closed when the activity is finished for good.
 */
class AppState(
    private val session: CoreSession,
    caps: Deferred<DeviceCapsParams>,
) : ViewModel() {
    var start: Start by mutableStateOf(Start.Loading)
        private set

    /** The one open Versions view, shared by the title page, the Versions screen and the player. */
    val versions = VersionsHolder(session, caps, viewModelScope)

    private var homeView: HomeView? = null

    /** Opened on first use (main thread only), so a launch that never reaches Home never subscribes. */
    val home: HomeView get() = homeView ?: session.home().also { homeView = it }
    val tvHome = TvHomeState()

    init {
        viewModelScope.launch { start = decide() }
    }

    /** Looks at the installed add-ons once the core runs: none means this is a new user. */
    private suspend fun decide(): Start {
        snapshotFlow { session.health }.first { it == CoreHealth.Running }
        val addons = session.addons()
        return try {
            addons.opened.await()
            snapshotFlow { addons.status }.first { it !is ViewStatus.Opening }
            if (addons.snapshot?.addons?.isEmpty() == true) Start.Onboarding else Start.Home
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            // Home shows its own explanation when the core cannot answer.
            Start.Home
        } finally {
            addons.close()
        }
    }

    override fun onCleared() {
        versions.close()
        homeView?.close()
    }
}
