package app.marquee.android

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import app.marquee.core.client.CoreLog
import app.marquee.core.session.CoreHealth
import app.marquee.core.session.CoreSession
import app.marquee.core.views.HomeView
import app.marquee.core.views.ViewStatus
import app.marquee.ui.tv.TvHomeState
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull

/** Where the app starts: a new user installs a catalog first, everyone else lands on Home. */
enum class Start { Loading, Onboarding, Home }

/**
 * What lives as long as the activity: the start decision, and Home, which stays open under the
 * pages above it so Back finds its rows, scroll positions and focus exactly as they were. The
 * core's view is closed when the activity is finished for good.
 */
class AppState(
    private val session: CoreSession,
    caps: CapsSource,
) : ViewModel() {
    var start: Start by mutableStateOf(Start.Loading)
        private set

    /** The one open Versions view, shared by the title page, the Versions screen and the player. */
    val versions = VersionsHolder(session, caps, viewModelScope)

    private var homeView: HomeView? = null

    /** Opened on first use (main thread only), so a launch that never reaches Home never subscribes. */
    val home: HomeView get() = homeView ?: session.home().also { homeView = it }
    val tvHome = TvHomeState()

    /** Where [decide] waits, for the log when it takes too long. */
    private var step = "waiting for the core"

    init {
        viewModelScope.launch {
            // Never a blank screen for good: past the limit Home opens, and says why if the core is silent.
            start = withTimeoutOrNull(START_WAIT_MS) { decide() } ?: Start.Home.also {
                CoreLog.w("start: no decision in $START_WAIT_MS ms at \"$step\", opening Home")
            }
        }
    }

    /** Looks at the installed addons once the core runs: none means this is a new user. */
    private suspend fun decide(): Start {
        snapshotFlow { session.health }.first { it == CoreHealth.Running }
        step = "opening the addons view"
        CoreLog.d("start: the core runs")
        val addons = session.addons()
        return try {
            addons.opened.await()
            step = "waiting for the addons"
            CoreLog.d("start: addons view opened")
            snapshotFlow { addons.status }.first { it !is ViewStatus.Opening }
            val start = if (addons.snapshot?.addons?.isEmpty() == true) Start.Onboarding else Start.Home
            CoreLog.d("start: $start")
            start
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
        versions.closeAll()
        homeView?.close()
    }
}

/** How long the start decision may wait for the core before Home opens anyway. */
private const val START_WAIT_MS = 10_000L
