package app.marquee.android

import android.content.Context
import android.os.SystemClock
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.ViewModel
import androidx.lifecycle.createSavedStateHandle
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import app.marquee.core.ErrorCopy
import app.marquee.core.Format
import app.marquee.core.PlayPick
import app.marquee.core.VersionsCopy
import app.marquee.core.client.CoreLog
import app.marquee.core.model.EmptyKind
import app.marquee.core.model.NextEpisode
import app.marquee.core.model.SubtitleLayout
import app.marquee.core.model.SubtitleSize
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
import app.marquee.player.PlayerLifecycle
import app.marquee.player.ProfileSetting
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
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import java.util.Locale

/** How long a recommendation stands unchanged before it plays (the web player waits the same). */
private const val SETTLE_MS = 3_000L

/** After an episode ends: how long the next one's versions have to show a pick before Versions opens instead. */
private const val BINGE_WAIT_MS = 6_000L

/**
 * Whether this player starts at the beginning: [Route.Player.startOver], once. The route is kept with the back stack,
 * so a player made again for it (the app back after the system ended its process) would start the film over and lose
 * the viewer's place; [saved] outlives that and remembers the start over was used.
 */
internal fun startsOver(
    route: Route.Player,
    saved: SavedStateHandle,
): Boolean {
    if (!route.startOver || saved.get<Boolean>(STARTED_OVER) == true) return false
    saved[STARTED_OVER] = true
    return true
}

private const val STARTED_OVER = "startedOver"

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
    saved: SavedStateHandle,
) : ViewModel() {
    private val host = PlayerHost(context.applicationContext, DeviceCaps.deviceLanguage(Locale.getDefault()))
    private val prefs: SettingsView = session.settings()

    /** The title's art for the loading screen (its backdrop and logo). */
    private val art = session.title(route.kind, route.id)
    private var finished = false

    val controller =
        PlaybackController(
            player = host.player,
            scope = viewModelScope,
            title = route.title,
            episode = Format.parseVideo(route.videoId)?.let { Format.episodeLabel(it.season, it.episode) },
            artwork = null,
            openedAtMs = SystemClock.elapsedRealtime(),
            subtitleDelay = host.subtitleDelay,
            startOver = startsOver(route, saved),
        )

    /** Start the next episode by itself when one ends. */
    val autoplayNext: Boolean get() = prefs.snapshot?.autoplayNext == true

    /** The picker's Size: shown at once, and kept as the profile's setting, as Settings sets it. */
    private val sizes =
        ProfileSetting(
            viewModelScope,
            SubtitleSize.Medium,
            write = { size ->
                session.ops
                    .setPrefs(buildJsonObject { put("subtitleSize", JsonPrimitive(size.wire)) })
                    .subtitleSize
            },
            show = { controller.subtitleSize = it },
        )

    fun setSubtitleSize(size: SubtitleSize) = sizes.pick(size)

    /** The second line's Layout and Only when paused (plans/dual-subtitles.md 5.1), as Settings sets them. */
    private val layouts =
        ProfileSetting(
            viewModelScope,
            SubtitleLayout.Stacked,
            write = { layout ->
                session.ops
                    .setPrefs(buildJsonObject { put("subtitleLayout", JsonPrimitive(layout.wire)) })
                    .subtitleLayout
            },
            show = { controller.subtitleLayout = it },
        )
    private val onPause =
        ProfileSetting(
            viewModelScope,
            false,
            write = { on ->
                session.ops.setPrefs(buildJsonObject { put("secondOnPause", JsonPrimitive(on)) }).secondOnPause
            },
            show = { controller.secondOnPause = it },
        )

    fun setSubtitleLayout(layout: SubtitleLayout) = layouts.pick(layout)

    fun setSecondOnPause(on: Boolean) = onPause.pick(on)

    /** The Spoken language row with no track of it here: the profile keeps it as its second line. */
    fun keepSpoken() {
        viewModelScope.launch {
            runCatching { session.ops.setPrefs(buildJsonObject { put("secondSubtitle", JsonPrimitive("audio")) }) }
                .onFailure { CoreLog.w("the spoken second line was not kept: ${it.message}") }
        }
    }

    init {
        viewModelScope.launch { start() }
        // Subtitles show at the profile's size, and follow it when Settings change it.
        viewModelScope.launch {
            snapshotFlow {
                prefs.snapshot?.subtitleSize
            }.collect { sizes.fromProfile(it ?: SubtitleSize.Medium) }
        }
        // The second line's settings, followed when Settings change them.
        viewModelScope.launch {
            snapshotFlow { prefs.snapshot }.collect { p ->
                layouts.fromProfile(p?.subtitleLayout ?: SubtitleLayout.Stacked)
                onPause.fromProfile(p?.secondOnPause == true)
                controller.secondPref = p?.secondSubtitle
            }
        }
        viewModelScope.launch {
            snapshotFlow { art.header?.details }.collect {
                controller.backdrop = it?.background
                controller.logo = it?.logo
            }
        }
        // The stream server moved while the app was away: the play opens again where it was.
        viewModelScope.launch { session.serverMoved.collect { controller.serverMoved() } }
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
        val choice = if (waiting) withTimeoutOrNull(BINGE_WAIT_MS) { choose(view) } else choose(view)
        if (choice == null) {
            if (controller.stage != PlaybackController.Stage.Failed &&
                waiting
            ) {
                controller.fail("No version was ready in time")
            }
            return
        }
        controller.mark("versions")
        controller.showStatus(choice.row?.let(VersionsCopy::starting) ?: "Starting")
        val ref = Format.parseVideo(route.videoId)
        try {
            val playback =
                session.playback(
                    versions = view,
                    key = choice.key,
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
            // The version could not be opened (no longer listed): the reason under the headline, and Try
            // again opens it again (review 2026-10-20 r3 P3-9).
            controller.cannotOpen(ErrorCopy.words(e)) {
                viewModelScope.launch { start() }
            }
        }
    }

    /** What to open: the key, and its row when the list has it (for the "Starting" line). */
    private data class Choice(
        val key: String,
        val row: VersionRow?,
    )

    /** The version to play, or null after the controller was told why there is none. */
    private suspend fun choose(view: VersionsView): Choice? {
        val chosen = CompletableDeferred<Choice?>()
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
                                chosen.complete(Choice(pick.row.key, pick.row))
                                return@collectLatest
                            }

                            is PlayPick.Open -> {
                                chosen.complete(Choice(pick.key, null))
                                return@collectLatest
                            }

                            is PlayPick.None -> {
                                // The core's state names the card's actions; Try again asks the addons again.
                                controller.fail(pick.reason, pick.kind, pick.key) {
                                    view.retry()
                                    viewModelScope.launch { start() }
                                }
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

    /** Reports where the player is and lets go of it; safe to call twice. */
    fun shutdown() {
        if (finished) return
        finished = true
        controller.release()
        host.release()
        prefs.close()
        art.close()
    }

    override fun onCleared() = shutdown()
}

/**
 * The Player route: the player of the current form with the window effects it needs, and the way
 * out. Leaving pauses first, so the sound stops with the tap and not after the fade.
 *
 * @param onVersions the Versions screen of this video, over the player, which pauses.
 * @param onTitle the title page of a series, for its episodes; null for a movie.
 * @param onNext the next episode (it replaces the player).
 * @param onOffer the version the failure card offers, by its key (it replaces the player).
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
    onOffer: (String) -> Unit,
    /** The failure card's Settings: the addons or the accounts, by the core's state. */
    onSettings: (EmptyKind) -> Unit = {},
) {
    val context = LocalContext.current
    val holder =
        viewModel<PlayerHolder>(
            key = "player:$route",
            factory =
                viewModelFactory {
                    initializer { PlayerHolder(session, app.versions, route, context, createSavedStateHandle()) }
                },
        )
    val controller = holder.controller
    // Leaving stops the sound; a start under way goes on in the background (plans/player-polish.md 2c).
    PlayerLifecycle(controller)
    // A page over the player (Versions, an install link) leaves it paused, and nothing plays it from under
    // there: not the remote's Play key through the media session, nor picture in picture.
    DisposableEffect(controller) {
        controller.covered = false
        onDispose { controller.covered = true }
    }
    val window = rememberPlayerWindow(controller, phone = form == Form.Phone)
    val leave = {
        controller.pause()
        onBack()
    }
    val versions = {
        controller.pause()
        onVersions()
    }
    val next: (NextEpisode, String?) -> Unit = { episode, binge ->
        holder.shutdown()
        onNext(episode, binge)
    }
    val offer: (String) -> Unit = { key ->
        holder.shutdown()
        onOffer(key)
    }
    if (form == Form.Tv) {
        TvPlayer(
            controller,
            holder.autoplayNext,
            versions,
            onTitle,
            next,
            leave,
            active = active,
            onOffer = offer,
            onSubtitleSize = holder::setSubtitleSize,
            onSubtitleLayout = holder::setSubtitleLayout,
            onSecondOnPause = holder::setSecondOnPause,
            onSpokenKept = holder::keepSpoken,
            onSettings = onSettings,
        )
    } else {
        PhonePlayer(
            controller,
            holder.autoplayNext,
            window.inPip,
            versions,
            onTitle,
            next,
            window::enterPip,
            leave,
            onOffer = offer,
            onSubtitleSize = holder::setSubtitleSize,
            onSubtitleLayout = holder::setSubtitleLayout,
            onSecondOnPause = holder::setSecondOnPause,
            onSpokenKept = holder::keepSpoken,
            onSettings = onSettings,
            brightness = window.brightness,
        )
    }
}
