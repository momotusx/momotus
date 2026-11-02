package app.marquee.core.views

import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshots.SnapshotStateList
import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreLog
import app.marquee.core.model.AddonCatalogSnapshot
import app.marquee.core.model.AddonsSnapshot
import app.marquee.core.model.CalendarSnapshot
import app.marquee.core.model.DebridLoginSnapshot
import app.marquee.core.model.DebridRecord
import app.marquee.core.model.DebridSnapshot
import app.marquee.core.model.DiscoverSnapshot
import app.marquee.core.model.DownloadsSnapshot
import app.marquee.core.model.Episode
import app.marquee.core.model.Hero
import app.marquee.core.model.HomeOffline
import app.marquee.core.model.HomeRow
import app.marquee.core.model.HomeSnapshot
import app.marquee.core.model.HomeUpdate
import app.marquee.core.model.ImportSnapshot
import app.marquee.core.model.LibrarySnapshot
import app.marquee.core.model.LibrarySort
import app.marquee.core.model.PairingSnapshot
import app.marquee.core.model.PlaybackSnapshot
import app.marquee.core.model.Prefs
import app.marquee.core.model.ProfilesSnapshot
import app.marquee.core.model.RowState
import app.marquee.core.model.SearchSnapshot
import app.marquee.core.model.SignInSnapshot
import app.marquee.core.model.SourceKind
import app.marquee.core.model.Tile
import app.marquee.core.model.TitleHeader
import app.marquee.core.model.TitleSnapshot
import app.marquee.core.model.TitleState
import app.marquee.core.model.TitleUpdate
import app.marquee.core.model.TraktLoginSnapshot
import app.marquee.core.model.TraktStatus
import app.marquee.core.model.VersionsSnapshot
import app.marquee.core.model.applyDiffs
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.serialization.KSerializer
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.put

private val NO_PARAMS = JsonObject(emptyMap())

/** Home: Continue Watching first, then one row per catalog, each row arriving on its own. */
class HomeView(
    client: CoreClient,
    scope: CoroutineScope,
) : LiveView<HomeSnapshot, HomeUpdate>(
        client,
        scope,
        "home",
        NO_PARAMS,
        HomeSnapshot.serializer(),
        HomeUpdate.serializer(),
    ) {
    val rows: SnapshotStateList<HomeRow> = mutableStateListOf()

    /** The hero carousel's pages, in the core's order (plans/appletv-polish.md H1). */
    var heroes: List<Hero> by mutableStateOf(emptyList())
        private set

    /** The first page: what the TV's start focus and the app's checks ask about. */
    val hero: Hero? get() = heroes.firstOrNull()

    /** No network and downloads that play (plans/offline-downloads.md 3.5); null otherwise. */
    var offline: HomeOffline? by mutableStateOf(null)
        private set

    override fun touch() {
        val (h, o, r) = Triple(heroes, offline, rows.toList())
        heroes = emptyList()
        heroes = h
        offline = null
        offline = o
        rows.clear()
        rows.addAll(r)
    }

    override fun applySnapshot(snapshot: HomeSnapshot) {
        heroes = snapshot.header.heroes
        offline = snapshot.header.offline
        rows.clear()
        rows.addAll(snapshot.rows)
        logFailures()
    }

    /** The rows whose failure is in the log: each failure once, again only after the row loaded or loads anew. */
    private val failedLogged = mutableSetOf<String>()

    /** A failed row says only "This row did not load": why goes to the log, without any address in it. */
    private fun logFailures() {
        val failed = rows.mapNotNull { row -> (row.state as? RowState.Failed)?.let { row.rowId to it.reason } }
        failedLogged.retainAll(failed.map { it.first }.toSet())
        for ((rowId, reason) in failed) {
            if (failedLogged.add(
                    rowId,
                )
            ) {
                CoreLog.w("home row $rowId did not load: ${reason.replace(ADDRESS, "<address>")}")
            }
        }
    }

    /** A failed catalog row's Try again: only that catalog is asked again (the row turns loading, then answers). */
    fun retryRow(rowId: String) = post(command("retryRow") { put("rowId", rowId) })

    override fun applyUpdates(updates: List<HomeUpdate>) {
        for (update in updates) {
            when (update) {
                is HomeUpdate.Header -> {
                    heroes = update.header.heroes
                    offline = update.header.offline
                }

                is HomeUpdate.Rows -> {
                    rows.applyDiffs(update.diffs)
                }
            }
        }
        logFailures()
    }
}

/** A web address in a core's words (the core already takes the keys out; the log keeps no host either). */
private val ADDRESS = Regex("""\b[a-zA-Z][a-zA-Z0-9+.-]*://[^\s)]+""")

/** One title: header (details, play target, seasons) and the episodes of the selected season. */
class TitleView(
    client: CoreClient,
    scope: CoroutineScope,
    kind: String,
    id: String,
) : LiveView<TitleSnapshot, TitleUpdate>(
        client,
        scope,
        "title",
        JsonObject(mapOf("kind" to JsonPrimitive(kind), "id" to JsonPrimitive(id))),
        TitleSnapshot.serializer(),
        TitleUpdate.serializer(),
    ) {
    var header: TitleHeader? by mutableStateOf(null)
        private set
    val episodes: SnapshotStateList<Episode> = mutableStateListOf()

    /** "More like this": empty until it lands, and for a kids profile. */
    var similar: List<Tile> by mutableStateOf(emptyList())
        private set

    /** Why the page cannot show: the title's own failure, or the view's before any header came ([retry] asks again). */
    val failure: String?
        get() =
            (header?.state as? TitleState.Failed)?.reason
                ?: (status as? ViewStatus.Failed)?.reason?.takeIf { header == null }

    override fun touch() {
        val (h, e, s) = Triple(header, episodes.toList(), similar)
        header = null
        header = h
        episodes.clear()
        episodes.addAll(e)
        similar = emptyList()
        similar = s
    }

    override fun applySnapshot(snapshot: TitleSnapshot) {
        header = snapshot.header
        episodes.clear()
        episodes.addAll(snapshot.episodes)
        similar = snapshot.similar
    }

    override fun applyUpdates(updates: List<TitleUpdate>) {
        for (update in updates) {
            when (update) {
                is TitleUpdate.Header -> header = update.header
                is TitleUpdate.Episodes -> episodes.applyDiffs(update.diffs)
                is TitleUpdate.Similar -> if (update.tiles != similar) similar = update.tiles
            }
        }
    }

    /**
     * The page shows or stops showing: the core asks TMDB and Trakt only while it shows
     * (plans/title-page-ratings-genres-similar.md), so send it on resume and on pause or leaving.
     */
    fun setVisible(visible: Boolean) {
        this.visible = visible
        post(command("titleVisible") { put("visible", visible) })
    }

    /** The last [setVisible], told again to a fresh subscription (Try again, a desync), which starts hidden. */
    @Volatile private var visible: Boolean? = null

    override fun replayed() = listOfNotNull(visible?.let { v -> command("titleVisible") { put("visible", v) } })

    fun selectSeason(season: Int) = post(command("selectSeason") { put("season", season) })

    fun setInLibrary(value: Boolean) = post(command("setInLibrary") { put("value", value) })

    fun setWatched(
        videoId: String,
        value: Boolean,
    ) = post(
        command("setWatched") {
            put("videoId", videoId)
            put("value", value)
        },
    )
}

/** A view whose every push is a whole snapshot: the latest wins. */
open class RecordView<S>(
    client: CoreClient,
    scope: CoroutineScope,
    view: String,
    params: JsonObject,
    serializer: KSerializer<S>,
) : LiveView<S, S>(client, scope, view, params, serializer, serializer) {
    var snapshot: S? by mutableStateOf(null)
        private set

    private val latestFlow = MutableStateFlow<S?>(null)

    /**
     * The same snapshot as a flow that hands a new collector the value it holds now: what [current] reads.
     */
    val latest: StateFlow<S?> get() = latestFlow

    /**
     * The snapshot as a screen reads it in composition: the latest one, read again once the screen is attached. A
     * snapshot written while the window's first composition runs reaches no recomposer yet, so a screen that read
     * [snapshot] alone stayed empty until the next change. [snapshot] stays right outside composition (callbacks,
     * effects, `snapshotFlow`).
     */
    @Composable
    fun current(): S? = latest.collectAsState().value

    /** How many opening snapshots arrived: one per subscription, so a new core's first one shows. */
    var opens by mutableIntStateOf(0)
        private set

    override fun applySnapshot(snapshot: S) {
        this.snapshot = snapshot
        latestFlow.value = snapshot
        opens++
    }

    override fun applyUpdates(updates: List<S>) {
        updates.lastOrNull()?.let {
            snapshot = it
            latestFlow.value = it
        }
    }
}

class SearchView(
    client: CoreClient,
    scope: CoroutineScope,
) : RecordView<SearchSnapshot>(client, scope, "search", NO_PARAMS, SearchSnapshot.serializer()) {
    /** The core debounces; send on every change. */
    fun setQuery(query: String) = post(command("setQuery") { put("query", query) })

    /** The user opened a result: remember the query. */
    fun remember() = post(command("remember"))

    /** "Clear" on the recent searches: the profile forgets them. */
    fun clearRecent() = post(command("clearRecent"))
}

/** What `select` does with the genre: leave it, clear it, or set it. */
sealed interface GenreChange {
    data object Keep : GenreChange

    data object Clear : GenreChange

    data class To(
        val genre: String,
    ) : GenreChange
}

class DiscoverView(
    client: CoreClient,
    scope: CoroutineScope,
) : RecordView<DiscoverSnapshot>(client, scope, "discover", NO_PARAMS, DiscoverSnapshot.serializer()) {
    /**
     * Changes the kind, service, sort, genre or decade (plans/discover-v2.md section 4); a kind and a genre
     * with nothing else, or a catalog, is a link the core starts from a clean slate. The service and the
     * decade take a [GenreChange] too: keep, clear (All, Any year) or set.
     */
    fun select(
        kind: String? = null,
        catalog: String? = null,
        genre: GenreChange = GenreChange.Keep,
        platform: GenreChange = GenreChange.Keep,
        sort: String? = null,
        decade: GenreChange = GenreChange.Keep,
    ) = post(
        command("select") {
            kind?.let { put("kind", it) }
            catalog?.let { put("catalog", it) }
            sort?.let { put("sort", it) }
            for ((key, change) in listOf("genre" to genre, "platform" to platform, "decade" to decade)) {
                when (change) {
                    GenreChange.Keep -> Unit
                    GenreChange.Clear -> put(key, JsonNull)
                    is GenreChange.To -> put(key, change.genre)
                }
            }
        },
    )

    fun loadMore() = post(command("loadMore"))
}

class LibraryView(
    client: CoreClient,
    scope: CoroutineScope,
) : RecordView<LibrarySnapshot>(client, scope, "library", NO_PARAMS, LibrarySnapshot.serializer()) {
    /** `null` shows every type. */
    fun setFilter(kind: String?) =
        post(command("setFilter") { put("kind", kind?.let { JsonPrimitive(it) } ?: JsonNull) })

    fun setSort(sort: LibrarySort) =
        post(command("setSort") { put("sort", sort.name.replaceFirstChar { it.lowercase() }) })
}

/** Episodes of the library's series around [today], the local date (`YYYY-MM-DD`). */
class CalendarView(
    client: CoreClient,
    scope: CoroutineScope,
    today: String,
) : RecordView<CalendarSnapshot>(
        client,
        scope,
        "calendar",
        JsonObject(mapOf("today" to JsonPrimitive(today))),
        CalendarSnapshot.serializer(),
    ) {
    /** Sent at local midnight and on every return to the app, so the window follows the date. */
    fun setToday(today: String) = post(command("setToday") { put("today", today) })

    /** Fetches one series of [CalendarSnapshot.failed] again. */
    fun retrySeries(itemId: String) = post(command("retry") { put("itemId", itemId) })
}

class AddonsView(
    client: CoreClient,
    scope: CoroutineScope,
) : RecordView<AddonsSnapshot>(client, scope, "addons", NO_PARAMS, AddonsSnapshot.serializer())

/**
 * This device's downloads (plans/offline-downloads.md 3.4): storage, settings, what runs and what is kept. One
 * stays open for the life of the process (the keeper reads `active` from it); screens open their own.
 */
class DownloadsView(
    client: CoreClient,
    scope: CoroutineScope,
) : RecordView<DownloadsSnapshot>(client, scope, "downloads", NO_PARAMS, DownloadsSnapshot.serializer())

/**
 * Find addons: Cinemeta's Official and Community lists, read at open. [fresh] waits for the network
 * instead of the addon cache (Refresh and Try again close the view and open it with it).
 */
class AddonCatalogView(
    client: CoreClient,
    scope: CoroutineScope,
    fresh: Boolean,
) : RecordView<AddonCatalogSnapshot>(
        client,
        scope,
        "addonCatalog",
        if (fresh) JsonObject(mapOf("fresh" to JsonPrimitive(true))) else NO_PARAMS,
        AddonCatalogSnapshot.serializer(),
    )

class SettingsView(
    client: CoreClient,
    scope: CoroutineScope,
) : RecordView<Prefs>(client, scope, "settings", NO_PARAMS, Prefs.serializer())

/** The device's debrid accounts and the records the app has not stored yet. */
class DebridView(
    client: CoreClient,
    scope: CoroutineScope,
    /** Every snapshot's pending records, on the state thread, as they arrive (not once per frame). */
    private val onPending: (List<DebridRecord>) -> Unit,
) : RecordView<DebridSnapshot>(client, scope, "debrid", NO_PARAMS, DebridSnapshot.serializer()) {
    override fun applySnapshot(snapshot: DebridSnapshot) {
        super.applySnapshot(snapshot)
        onPending(snapshot.pending)
    }

    override fun applyUpdates(updates: List<DebridSnapshot>) {
        super.applyUpdates(updates)
        updates.lastOrNull()?.let { onPending(it.pending) }
    }
}

/** A sign-in by code for one service: the code and its QR, then done or why not. */
class DebridLoginView(
    client: CoreClient,
    scope: CoroutineScope,
    service: String,
) : RecordView<DebridLoginSnapshot>(
        client,
        scope,
        "debridLogin",
        JsonObject(mapOf("service" to JsonPrimitive(service))),
        DebridLoginSnapshot.serializer(),
    )

/** The profile's Trakt status, open for the profile's session; each push may offer a record to store. */
class TraktView(
    client: CoreClient,
    scope: CoroutineScope,
    /** Every snapshot, on the state thread, as it arrives (not once per frame). */
    private val onStatus: (TraktStatus) -> Unit,
) : RecordView<TraktStatus>(client, scope, "trakt", NO_PARAMS, TraktStatus.serializer()) {
    override fun applySnapshot(snapshot: TraktStatus) {
        super.applySnapshot(snapshot)
        onStatus(snapshot)
    }

    override fun applyUpdates(updates: List<TraktStatus>) {
        super.applyUpdates(updates)
        updates.lastOrNull()?.let(onStatus)
    }
}

/** A Trakt sign-in by code (one at a time): the code and its QR, then done or why not. */
class TraktLoginView(
    client: CoreClient,
    scope: CoroutineScope,
) : RecordView<TraktLoginSnapshot>(client, scope, "traktLogin", NO_PARAMS, TraktLoginSnapshot.serializer())

/** The picker's view: the session's profile, whether it is locked, and every profile. */
class ProfilesView(
    client: CoreClient,
    scope: CoroutineScope,
) : RecordView<ProfilesSnapshot>(client, scope, "profiles", NO_PARAMS, ProfilesSnapshot.serializer())

class ImportView(
    client: CoreClient,
    scope: CoroutineScope,
) : RecordView<ImportSnapshot>(client, scope, "import", NO_PARAMS, ImportSnapshot.serializer())

class VersionsView(
    client: CoreClient,
    scope: CoroutineScope,
    params: JsonObject,
) : RecordView<VersionsSnapshot>(client, scope, "versions", params, VersionsSnapshot.serializer()) {
    /**
     * Settings was opened from the page that shows this view (an addon or an account to add): the page asks the core
     * again when it resumes, so what was added shows without a press.
     */
    var askAgain = false

    /** Send on the first focus move: the pick under the cursor never changes after. */
    fun freeze() {
        CoreLog.d("versions: the pick is frozen")
        post(command("freeze"))
    }

    /** Lists one kind of version only, or every one ([kind] null); the core freezes the pick (`setKind`). */
    fun setKind(kind: SourceKind?) {
        post(command("setKind") { put("kind", kind?.wire?.let(::JsonPrimitive) ?: JsonNull) })
    }
}

/**
 * The `failOver` command for the source the player loaded as [attempt]. An unknown attempt names
 * none, and the core then acts on the current source. [kind] (with the HTTP [status]) says what
 * failed, and the core words it for the attempt list; [error] counts only for `other`.
 */
internal fun failOverCommand(
    error: String,
    attempt: Long?,
    kind: String,
    status: Int?,
): JsonObject =
    command("failOver") {
        put("error", error)
        put("kind", kind)
        if (status != null) put("status", status)
        if (attempt != null) put("attempt", attempt)
    }

class PlaybackView(
    client: CoreClient,
    scope: CoroutineScope,
    params: JsonObject,
) : RecordView<PlaybackSnapshot>(client, scope, "playback", params, PlaybackSnapshot.serializer()) {
    /**
     * Report where the player is. The core takes unsigned integers, so a duration the player does
     * not know yet (Media3 reports a negative `C.TIME_UNSET`) skips the report; and a report before
     * the first frame would overwrite the resume point with 0, so callers send none until then.
     */
    fun progress(
        positionMs: Long,
        durationMs: Long,
        /** The viewer paused (or it ended): a Trakt scrobble pauses; playing again starts it. */
        paused: Boolean = false,
    ) {
        if (positionMs < 0 || durationMs <= 0) return
        post(
            command("progress") {
                put("positionMs", positionMs)
                put("durationMs", durationMs)
                put("paused", paused)
            },
        )
    }

    /**
     * [attempt] is the source that failed (the snapshot's `attempt` when it was loaded), so the core
     * ignores a late error, even from another source of the same key.
     */
    fun failOver(
        error: String,
        attempt: Long?,
        kind: String,
        status: Int?,
    ) = post(failOverCommand(error, attempt, kind, status))

    /**
     * Tell the core how long the version that plays is, as soon as the player knows: it looks up
     * skip segments by it. [key] is the playing version's key from the snapshot; a report for another
     * key is ignored, and a repeat is harmless.
     */
    fun duration(
        key: String,
        durationMs: Long,
    ) {
        if (durationMs <= 0) return
        post(
            command("duration") {
                put("key", key)
                put("durationMs", durationMs)
            },
        )
    }

    /**
     * The viewer's subtitle pick (a core track's `url`) or Off (null): the start subtitle clears, and the
     * title remembers the choice for its next plays (plans/m13-subtitle-languages.md 6).
     */
    fun subtitleChosen(
        url: String?,
        /** A track inside the file: no [url], its language tag, which the title remembers as a core track's. */
        language: String? = null,
    ) = post(
        command("subtitleChosen") {
            put("url", url)
            if (language != null) put("language", language)
        },
    )

    /**
     * The viewer's second subtitle line (a core track's `url`) or Off (null), plans/dual-subtitles.md 3.1: the core's
     * `autoSecondSubtitle` clears for the rest of the play, and the profile keeps its language, the spoken one for the
     * Spoken language row ([spoken]).
     */
    fun secondSubtitleChosen(
        url: String?,
        spoken: Boolean,
    ) = post(
        command("secondSubtitleChosen") {
            put("url", url)
            put("spoken", spoken)
        },
    )

    /**
     * The viewer's delay for a core track ([url]), in ms (positive shows the text later): the title remembers it
     * for this version and video, and the track's `delayMs` starts there next time.
     */
    fun subtitleDelay(
        url: String,
        ms: Long,
    ) = post(
        command("subtitleDelay") {
            put("url", url)
            put("ms", ms)
        },
    )

    /**
     * "Try again" on the failure card (the core's `retry`): the same versions again from the first pick. Not
     * [retry], which opens a failed view again.
     */
    fun retryPlay() = post(command("retry"))

    /**
     * The player waits for data mid-play ([on]), or plays again, at [positionMs] (plans/play-every-version.md 2e):
     * the core may move on to the next version, or offer Choose another version over the stalled frame.
     */
    fun stalled(
        on: Boolean,
        positionMs: Long,
    ) = post(
        command("stalled") {
            put("stalled", on)
            put("positionMs", positionMs.coerceAtLeast(0))
        },
    )
}

/** `name` is how the account's other devices will show this one. */
class PairingView(
    client: CoreClient,
    scope: CoroutineScope,
    server: String,
    name: String,
    /** `phone`, `tablet` or `tv`, sealed in the roster: only a phone or tablet approves others (D6). */
    form: String? = null,
) : RecordView<PairingSnapshot>(
        client,
        scope,
        "pairing",
        JsonObject(
            buildMap {
                put("server", JsonPrimitive(server))
                put("name", JsonPrimitive(name))
                form?.let { put("form", JsonPrimitive(it)) }
            },
        ),
        PairingSnapshot.serializer(),
    )

/**
 * A relay's brokered sign-in (`signIn`), or Apple's revoke (`appleRevoke`), returning to `momotus://auth`
 * (plans/core-protocol.md "Sign-in"). Every phase reaches [onPhase] on the state thread as it arrives.
 */
class SignInView(
    client: CoreClient,
    scope: CoroutineScope,
    server: String,
    provider: String,
    revoke: Boolean,
    private val onPhase: (SignInSnapshot) -> Unit,
) : RecordView<SignInSnapshot>(
        client,
        scope,
        if (revoke) "appleRevoke" else "signIn",
        JsonObject(
            buildMap {
                put("server", JsonPrimitive(server))
                if (!revoke) put("provider", JsonPrimitive(provider))
                put("return", JsonPrimitive(RETURN))
            },
        ),
        SignInSnapshot.serializer(),
    ) {
    override fun applySnapshot(snapshot: SignInSnapshot) {
        super.applySnapshot(snapshot)
        onPhase(snapshot)
    }

    override fun applyUpdates(updates: List<SignInSnapshot>) {
        super.applyUpdates(updates)
        updates.lastOrNull()?.let(onPhase)
    }

    /** The redirect's fragment (after `#`), from this app's own auth session only. */
    fun returned(fragment: String) = post(command("returned") { put("fragment", JsonPrimitive(fragment)) })

    companion object {
        /** Where the relay sends the browser back: the app's own scheme (an Auth Tab's redirect scheme). */
        const val RETURN = "momotus://auth"
    }
}
