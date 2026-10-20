package app.marquee.android

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelStore
import androidx.lifecycle.ViewModelStoreOwner
import androidx.lifecycle.viewModelScope
import app.marquee.core.DefaultsCard
import app.marquee.core.InstallCards
import app.marquee.core.LinkInstall
import app.marquee.core.client.CoreLog
import app.marquee.core.session.CoreHealth
import app.marquee.core.session.CoreSession
import app.marquee.core.startsFresh
import app.marquee.core.views.HomeView
import app.marquee.core.views.ViewStatus
import app.marquee.ui.tv.TvHomeState
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull

/** Where the app starts: a new user chooses addons first, everyone else lands on Home. */
enum class Start { Loading, Onboarding, Home }

/**
 * What lives as long as the activity: the start decision, and Home, which stays open under the
 * pages above it so Back finds its rows, scroll positions and focus exactly as they were. The
 * core's view is closed when the activity is finished for good.
 */
class AppState(
    private val session: CoreSession,
    caps: CapsSource,
    /** The gate's shuts before this state existed (a new activity in a running process): nothing to reset. */
    shutsBefore: Int,
) : ViewModel() {
    var start: Start by mutableStateOf(Start.Loading)
        private set

    /**
     * Whether this process composed a back stack yet. The first one may be what the saved state brought
     * back after the process died (this state is new then, and survives a configuration change).
     */
    var stackSeen = false

    /** The one open Versions view, shared by the title page, the Versions screen and the player. */
    val versions = VersionsHolder(session, caps, viewModelScope)

    /** Find addons cards' Installing and failure states: they outlive the page, never the profile. */
    val installCards = InstallCards(viewModelScope)

    /** Adding an addon by its link (Settings, Find addons): it outlives the page, never the profile. */
    val linkInstall = LinkInstall(viewModelScope)

    /** Home's one-time recommended addons card: its answer is written on this scope, never the page's. */
    val defaultsCard = DefaultsCard(session.ops, viewModelScope)

    private var homeView: HomeView? = null

    /** Opened on first use (main thread only), so a launch that never reaches Home never subscribes. */
    val home: HomeView get() = homeView ?: session.home().also { homeView = it }
    var tvHome by mutableStateOf(TvHomeState())
        private set

    /**
     * Where the screens' view models live: one profile's worth. A switch or a lock clears it, so
     * every screen's views close (the core switches only with the picker open).
     */
    val screens =
        object : ViewModelStoreOwner {
            override val viewModelStore = ViewModelStore()
        }

    /** The gate's shut this state last answered. */
    var resetAt = shutsBefore
        private set

    /** A switch or a lock sent the app Home: the start decision no longer applies. */
    private var sentHome = false

    /**
     * A switch, a lock or a core that stopped: the screens, Home, Versions and the TV focus memory
     * belong to the old core or profile. A switch or a lock opens the next profile on Home, as a cold
     * start of a known user would; a restart keeps where the app started (onboarding stays onboarding).
     */
    fun resetProfile(
        shut: Int,
        home: Boolean,
    ) {
        resetAt = shut
        screens.viewModelStore.clear()
        versions.closeAll()
        homeView?.close()
        homeView = null
        tvHome = TvHomeState()
        installCards.forgetAll()
        linkInstall.forget()
        defaultsCard.forget()
        if (home) {
            sentHome = true
            start = Start.Home
        }
    }

    /** Onboarding is done: a restart from here on opens Home, not onboarding again. */
    fun onboarded() {
        if (start == Start.Onboarding) start = Start.Home
    }

    /** Where [decide] waits, for the log when it takes too long. */
    private var step = "waiting for the core"

    init {
        viewModelScope.launch {
            // Never a blank screen for good: past the limit Home opens, and says why if the core is silent.
            val decided =
                withTimeoutOrNull(START_WAIT_MS) { decide() } ?: Start.Home.also {
                    // The core's state then tells a slow open (still starting, no open time) from a missed change.
                    CoreLog.w(
                        "start: no decision in $START_WAIT_MS ms at \"$step\" (core ${session.health}, " +
                            "open ${session.openMillis} ms), opening Home",
                    )
                }
            // A switch or a lock meanwhile (a lock at start) already chose Home for the next profile.
            if (!sentHome) start = decided
        }
    }

    /**
     * Asks the addons view once the core runs: `fresh` (no list ever, no sync account, not a kids
     * profile) means a new user, who starts at onboarding until the defaults step is done.
     */
    private suspend fun decide(): Start {
        snapshotFlow { session.health }.first { it == CoreHealth.Running }
        step = "reading the addons view"
        CoreLog.d("start: the core runs")
        val start =
            try {
                if (startsFresh(session.addons())) Start.Onboarding else Start.Home
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                // Home shows its own explanation when the core cannot answer.
                Start.Home
            }
        CoreLog.d("start: $start")
        return start
    }

    override fun onCleared() {
        screens.viewModelStore.clear()
        versions.closeAll()
        homeView?.close()
    }
}

/** How long the start decision may wait for the core before Home opens anyway. */
private const val START_WAIT_MS = 10_000L
