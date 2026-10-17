package app.marquee.android

import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import app.marquee.core.Format
import app.marquee.core.model.Episode
import app.marquee.core.model.VersionRow
import app.marquee.core.route.Route
import app.marquee.core.session.CoreSession
import app.marquee.core.views.TitleView
import app.marquee.core.views.ViewStatus
import app.marquee.design.Form
import app.marquee.ui.phone.PhoneTitle
import app.marquee.ui.phone.PhoneVersions
import app.marquee.ui.tv.FocusMemory
import app.marquee.ui.tv.TvTitle
import app.marquee.ui.tv.TvVersions

/** A title's core view and its TV focus memory; both live as long as the page is on the back stack. */
class TitleHolder(
    session: CoreSession,
    kind: String,
    id: String,
) : ViewModel() {
    val view: TitleView = session.title(kind, id)
    val focus = FocusMemory()

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
    onPlay: (key: String, videoId: String, title: String) -> Unit,
    onVersions: (videoId: String, title: String) -> Unit,
    onPlayEpisode: (videoId: String, title: String) -> Unit,
    onAddSource: () -> Unit,
) {
    val holder =
        viewModel<TitleHolder>(
            key = "title:${route.kind}:${route.id}",
            factory = viewModelFactory { initializer { TitleHolder(session, route.kind, route.id) } },
        )
    val header = holder.view.header
    val name = header?.details?.name
    val videoId = header?.playTarget?.videoId
    // The line under Play is the recommended version of what Play starts, so open it early.
    val playKey = if (videoId != null && name != null) keyFor(route.kind, videoId, name) else null
    LaunchedEffect(playKey) { playKey?.let(app.versions::use) }
    val versions = playKey?.let(app.versions::view)
    val snapshot = versions?.snapshot
    val failed = versions?.status is ViewStatus.Failed
    val title = name.orEmpty()
    // A tap on an episode plays it (the player picks its version); its versions are in the hold menu.
    val episode: (Episode) -> Unit = { onPlayEpisode(it.videoId, title) }
    val episodeVersions: (Episode) -> Unit = { onVersions(it.videoId, title) }
    val play: (String, String) -> Unit = { key, id -> onPlay(key, id, title) }
    if (form == Form.Tv) {
        TvTitle(
            holder.view,
            snapshot,
            holder.focus,
            play,
            onVersions = { onVersions(it, title) },
            onAddSource = onAddSource,
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
            onAddSource = onAddSource,
            onEpisode = episode,
            onEpisodeVersions = episodeVersions,
            versionsFailed = failed,
        )
    }
}

@Composable
fun VersionsScreen(
    route: Route.Versions,
    app: AppState,
    form: Form,
    onPlay: (VersionRow) -> Unit,
    onAddSource: () -> Unit,
) {
    val key = keyFor(route.kind, route.videoId, route.title)
    LaunchedEffect(key) { app.versions.use(key) }
    val view = app.versions.view(key)
    val heading =
        key.episode?.let { "${route.title}, ${Format.episodeLabel(key.season ?: 0, it)}" } ?: route.title
    if (form == Form.Tv) {
        val focus = viewModel<VersionsFocus>(key = "versions:${route.videoId}").focus
        TvVersions(heading, view, focus, onPlay, onAddSource)
    } else {
        PhoneVersions(heading, view, onPlay, onAddSource)
    }
}
