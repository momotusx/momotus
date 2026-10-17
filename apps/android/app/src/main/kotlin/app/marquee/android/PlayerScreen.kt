package app.marquee.android

import android.content.Context
import android.os.SystemClock
import androidx.compose.runtime.Composable
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.LifecycleEventEffect
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import app.marquee.core.Format
import app.marquee.core.PlayPick
import app.marquee.core.VersionsCopy
import app.marquee.core.model.NextEpisode
import app.marquee.core.model.VersionRow
import app.marquee.core.pickVersion
import app.marquee.core.route.Route
import app.marquee.core.session.CoreSession
import app.marquee.core.views.SettingsView
import app.marquee.core.views.VersionsView
import app.marquee.core.views.ViewStatus
import app.marquee.design.Form
import app.marquee.player.PlaybackController
import app.marquee.player.PlayerHost
import app.marquee.ui.phone.PhonePlayer
import app.marquee.ui.tv.TvPlayer
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.filterNotNull
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull
import java.util.Locale

/** How long a recommendation stands unchanged before it plays (the web player waits the same). */
private const val SETTLE_MS = 3_000L

/** After an episode ends: how long the next one's versions have to show a pick before Versions opens instead. */
private const val BINGE_WAIT_MS = 6_000L

/**
 * The player of one Player route: the ExoPlayer, the controller that follows the core, and the
 * steps before them (find the ranked versions, pick one, open the core's playback). Lives as long
 * as the route is on the back stack, and lets go of the player when it leaves.
 */
class PlayerHolder(
    private val session: CoreSession,
    private val versions: VersionsHolder,
    private val route: Route.Player,
    context: Context,
) : ViewModel() {
    private val host = PlayerHost(context.applicationContext, DeviceCaps.deviceLanguage(Locale.getDefault()))
    private val prefs: SettingsView = session.settings()
    private var finished = false

    val controller =
        PlaybackController(
            host = host,
            scope = viewModelScope,
            title = route.title,
            episode = Format.parseVideo(route.videoId)?.let { Format.episodeLabel(it.season, it.episode) },
            artwork = null,
            openedAtMs = SystemClock.elapsedRealtime(),
        )

    /** Start the next episode by itself when one ends. */
    val autoplayNext: Boolean get() = prefs.snapshot?.autoplayNext == true

    init {
        viewModelScope.launch { start() }
        // The Versions button names the quality of what plays, also after a failover.
        viewModelScope.launch {
            val key = keyFor(route.kind, route.videoId, route.title, route.binge)
            snapshotFlow {
                val playing = controller.playingKey
                val snapshot = versions.view(key)?.snapshot
                (listOfNotNull(snapshot?.recommended) + snapshot?.rows.orEmpty())
                    .firstOrNull { it.key == playing }
                    ?.let { VersionsCopy.cell(it.quality) }
            }.collect { controller.versionLabel = it }
        }
    }

    private suspend fun start() {
        val key = keyFor(route.kind, route.videoId, route.title, route.binge)
        versions.use(key)
        val view = snapshotFlow { versions.view(key) }.filterNotNull().first()
        // After an episode the next one should start by itself: if no pick shows up in time, say so.
        val waiting = route.binge != null && route.key == null
        val row = if (waiting) withTimeoutOrNull(BINGE_WAIT_MS) { choose(view) } else choose(view)
        if (row == null) {
            if (controller.stage != PlaybackController.Stage.Failed &&
                waiting
            ) {
                controller.fail("No version was ready in time")
            }
            return
        }
        controller.mark("versions")
        controller.showStatus("Starting ${startingLine(row)}")
        val ref = Format.parseVideo(route.videoId)
        try {
            val playback =
                session.playback(
                    versions = view,
                    key = row.key,
                    kind = route.kind,
                    itemId = route.id,
                    videoId = route.videoId,
                    name = route.title,
                    poster = null,
                    season = ref?.season,
                    episode = ref?.episode,
                )
            controller.attach(playback)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            controller.fail(e.message ?: "The core could not start this version")
        }
    }

    /** The version to play, or null after the controller was told why there is none. */
    private suspend fun choose(view: VersionsView): VersionRow? {
        val chosen = CompletableDeferred<VersionRow?>()
        val watcher =
            viewModelScope.launch {
                snapshotFlow { view.snapshot to view.status }.collectLatest { (snapshot, status) ->
                    if (status is ViewStatus.Failed) {
                        controller.fail(status.reason)
                        chosen.complete(null)
                        return@collectLatest
                    }
                    var settled = false
                    while (true) {
                        when (val pick = pickVersion(snapshot, route.key, settled)) {
                            is PlayPick.Use -> {
                                chosen.complete(pick.row)
                                return@collectLatest
                            }

                            is PlayPick.None -> {
                                controller.fail(pick.reason)
                                chosen.complete(null)
                                return@collectLatest
                            }

                            is PlayPick.Wait -> {
                                controller.showStatus(pick.line)
                            }
                        }
                        if (settled || snapshot?.recommended == null) return@collectLatest
                        delay(SETTLE_MS)
                        settled = true
                    }
                }
            }
        try {
            return chosen.await()
        } finally {
            watcher.cancel()
        }
    }

    private fun startingLine(row: VersionRow): String =
        listOfNotNull(row.quality, row.sizeBytes?.let(Format::bytes)).joinToString(", ") + " from ${row.via}"

    /** Reports where the player is and lets go of it; safe to call twice. */
    fun shutdown() {
        if (finished) return
        finished = true
        controller.release()
        host.release()
        prefs.close()
    }

    override fun onCleared() = shutdown()
}

/**
 * The Player route: the player of the current form with the window effects it needs, and the way
 * out. Leaving pauses first, so the sound stops with the tap and not after the fade.
 *
 * @param onVersions the Versions screen of this video (it replaces the player).
 * @param onTitle the title page of a series, for its episodes; null for a movie.
 * @param onNext the next episode (it replaces the player).
 */
@Composable
fun PlayerScreen(
    route: Route.Player,
    session: CoreSession,
    app: AppState,
    form: Form,
    active: Boolean,
    onBack: () -> Unit,
    onVersions: () -> Unit,
    onTitle: (() -> Unit)?,
    onNext: (NextEpisode, String?) -> Unit,
) {
    val context = LocalContext.current
    val holder =
        viewModel<PlayerHolder>(
            key = "player:$route",
            factory = viewModelFactory { initializer { PlayerHolder(session, app.versions, route, context) } },
        )
    val controller = holder.controller
    LifecycleEventEffect(Lifecycle.Event.ON_STOP) { controller.onStop() }
    val window = rememberPlayerWindow(controller, phone = form == Form.Phone)
    val leave = {
        controller.pause()
        onBack()
    }
    val next: (NextEpisode, String?) -> Unit = { episode, binge ->
        holder.shutdown()
        onNext(episode, binge)
    }
    if (form == Form.Tv) {
        TvPlayer(controller, holder.autoplayNext, onVersions, onTitle, next, leave, active = active)
    } else {
        PhonePlayer(controller, holder.autoplayNext, window.inPip, onVersions, onTitle, next, window::enterPip, leave)
    }
}
