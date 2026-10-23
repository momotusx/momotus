package app.marquee.android

import androidx.activity.compose.ReportDrawnWhen
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalAccessibilityManager
import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.ViewModel
import androidx.lifecycle.createSavedStateHandle
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import app.marquee.core.Format
import app.marquee.core.TitleCopy
import app.marquee.core.client.TitleFacts
import app.marquee.core.model.Episode
import app.marquee.core.model.LearnedNote
import app.marquee.core.route.Route
import app.marquee.core.session.CoreSession
import app.marquee.core.views.TitleView
import app.marquee.core.views.ViewStatus
import app.marquee.design.Form
import app.marquee.design.Icons
import app.marquee.design.Marquee
import app.marquee.design.Toast
import app.marquee.ui.phone.PhoneTitle
import app.marquee.ui.phone.PhoneVersions
import app.marquee.ui.tv.FocusMemory
import app.marquee.ui.tv.TvTitle
import app.marquee.ui.tv.TvVersions
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch

/**
 * A title's core view and its TV focus memory; both live as long as the page is on the back stack.
 * The season to open on is selected once, when the seasons arrive: the one shown before the process
 * died (kept in [saved]), else [season] (a Calendar episode). From then on the season shown is
 * kept, so a restore brings back the user's pick rather than the route's.
 */
class TitleHolder(
    session: CoreSession,
    kind: String,
    id: String,
    season: Int?,
    saved: SavedStateHandle,
) : ViewModel() {
    val view: TitleView = session.title(kind, id)
    val focus = FocusMemory()

    init {
        val want = saved.get<Int>(SEASON) ?: season
        viewModelScope.launch {
            val header = snapshotFlow { view.header }.first { !it?.seasons.isNullOrEmpty() } ?: return@launch
            if (want != null && want in header.seasons && header.selectedSeason != want) view.selectSeason(want)
            snapshotFlow { view.header?.selectedSeason }.collect { if (it != null) saved[SEASON] = it }
        }
    }

    override fun onCleared() = view.close()
}

/** The Versions screen's TV focus memory. */
class VersionsFocus : ViewModel() {
    val focus = FocusMemory()
}

internal fun keyFor(
    kind: String,
    videoId: String,
    title: String,
    bingeGroup: String? = null,
): VersionsKey {
    val ref = Format.parseVideo(videoId)
    return VersionsKey(kind, videoId, title, season = ref?.season, episode = ref?.episode, bingeGroup = bingeGroup)
}

@Composable
fun TitleScreen(
    route: Route.Title,
    session: CoreSession,
    app: AppState,
    form: Form,
    onPlay: (key: String?, videoId: String, title: String) -> Unit,
    onVersions: (videoId: String, title: String) -> Unit,
    onPlayEpisode: (videoId: String, title: String) -> Unit,
) {
    val holder =
        viewModel<TitleHolder>(
            key = "title:${route.kind}:${route.id}",
            factory =
                viewModelFactory {
                    initializer { TitleHolder(session, route.kind, route.id, route.season, createSavedStateHandle()) }
                },
        )
    val header = holder.view.header
    val name = header?.details?.name
    // Fully drawn once the details show, when a start opens on a title page.
    ReportDrawnWhen { name != null }
    val videoId = header?.playTarget?.videoId
    // The line under Play is the recommended version of what Play starts, so open it early.
    val playKey = if (videoId != null && name != null) keyFor(route.kind, videoId, name) else null
    // The runtime and dates go with each video's view, so the bitrate and year checks need not wait for the meta.
    val facts = TitleFacts.of(header?.details, videoId, holder.view.episodes.toList())
    LaunchedEffect(playKey, facts) {
        app.versions.know(facts)
        playKey?.let(app.versions::use)
    }
    val versions = playKey?.let(app.versions::view)
    val snapshot = versions?.snapshot
    val failed = versions?.status is ViewStatus.Failed
    val title = name.orEmpty()
    // A tap on an episode plays it (the player picks its version); its versions are in the hold menu.
    // One that has not aired says when it does instead, in a passing note: a player could only fail.
    // Keyed by a count, so the same episode pressed again shows and says its note again.
    var note by remember { mutableStateOf<Pair<Int, String>?>(null) }
    val accessibility = LocalAccessibilityManager.current
    LaunchedEffect(note) {
        if (note == null) return@LaunchedEffect
        delay(
            accessibility?.calculateRecommendedTimeoutMillis(NOTE_MS, containsIcons = true, containsText = true)
                ?: NOTE_MS,
        )
        note = null
    }
    val episode: (Episode) -> Unit = {
        val airs = TitleCopy.airs(it)
        if (airs == null) onPlayEpisode(it.videoId, title) else note = (note?.first ?: 0) + 1 to airs
    }
    val episodeVersions: (Episode) -> Unit = { onVersions(it.videoId, title) }
    val play: (String?, String) -> Unit = { key, id -> onPlay(key, id, title) }
    Box(Modifier.fillMaxSize()) {
        if (form == Form.Tv) {
            TvTitle(
                holder.view,
                snapshot,
                holder.focus,
                play,
                onVersions = { onVersions(it, title) },
                onEpisode = episode,
                onEpisodeVersions = episodeVersions,
                versionsFailed = failed,
            )
        } else {
            PhoneTitle(
                holder.view,
                snapshot,
                play,
                onVersions = { onVersions(it, title) },
                onEpisode = episode,
                onEpisodeVersions = episodeVersions,
                versionsFailed = failed,
            )
        }
        note?.let { (id, words) ->
            key(id) {
                Toast(
                    words,
                    Icons.CalendarDays,
                    Modifier
                        .align(Alignment.BottomCenter)
                        .windowInsetsPadding(WindowInsets.navigationBars)
                        .padding(bottom = if (form == Form.Tv) Marquee.safe.bottom else Marquee.space.m),
                )
            }
        }
    }
}

/** How long a passing note stays (a short system toast's two seconds), unless accessibility asks for longer. */
private const val NOTE_MS = 2_000L

@Composable
fun VersionsScreen(
    route: Route.Versions,
    app: AppState,
    form: Form,
    onPlay: (String) -> Unit,
    onAddSource: () -> Unit,
    onAddAccount: () -> Unit,
    onBack: () -> Unit,
    onUndo: suspend (LearnedNote) -> Unit,
) {
    val key = keyFor(route.kind, route.videoId, route.title)
    LaunchedEffect(key) { app.versions.use(key) }
    val view = app.versions.view(key)
    val heading =
        key.episode?.let { "${route.title}, ${Format.episodeLabel(key.season ?: 0, it)}" } ?: route.title
    if (form == Form.Tv) {
        val focus = viewModel<VersionsFocus>(key = "versions:${route.videoId}").focus
        TvVersions(heading, view, focus, onPlay, onAddSource, onAddAccount, onBack, onUndo)
    } else {
        PhoneVersions(heading, view, onPlay, onAddSource, onAddAccount, onBack, onUndo)
    }
}

private const val SEASON = "season"
