package app.marquee.core.views

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshots.SnapshotStateList
import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreLog
import app.marquee.core.model.AddonsSnapshot
import app.marquee.core.model.CalendarSnapshot
import app.marquee.core.model.DebridLoginSnapshot
import app.marquee.core.model.DebridRecord
import app.marquee.core.model.DebridSnapshot
import app.marquee.core.model.DiscoverSnapshot
import app.marquee.core.model.Episode
import app.marquee.core.model.Hero
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
import app.marquee.core.model.SearchSnapshot
import app.marquee.core.model.TitleHeader
import app.marquee.core.model.TitleSnapshot
import app.marquee.core.model.TitleUpdate
import app.marquee.core.model.VersionsSnapshot
import app.marquee.core.model.applyDiffs
import kotlinx.coroutines.CoroutineScope
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
    var hero: Hero? by mutableStateOf(null)
        private set

    override fun applySnapshot(snapshot: HomeSnapshot) {
        hero = snapshot.header.hero
        rows.clear()
        rows.addAll(snapshot.rows)
    }

    override fun applyUpdates(updates: List<HomeUpdate>) {
        for (update in updates) {
            when (update) {
                is HomeUpdate.Header -> hero = update.header.hero
                is HomeUpdate.Rows -> rows.applyDiffs(update.diffs)
            }
        }
    }
}

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

    override fun applySnapshot(snapshot: TitleSnapshot) {
        header = snapshot.header
        episodes.clear()
        episodes.addAll(snapshot.episodes)
    }

    override fun applyUpdates(updates: List<TitleUpdate>) {
        for (update in updates) {
            when (update) {
                is TitleUpdate.Header -> header = update.header
                is TitleUpdate.Episodes -> episodes.applyDiffs(update.diffs)
            }
        }
    }

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

    /** How many opening snapshots arrived: one per subscription, so a new core's first one shows. */
    var opens by mutableIntStateOf(0)
        private set

    override fun applySnapshot(snapshot: S) {
        this.snapshot = snapshot
        opens++
    }

    override fun applyUpdates(updates: List<S>) {
        updates.lastOrNull()?.let { snapshot = it }
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
    fun select(
        kind: String? = null,
        catalog: String? = null,
        genre: GenreChange = GenreChange.Keep,
    ) = post(
        command("select") {
            kind?.let { put("kind", it) }
            catalog?.let { put("catalog", it) }
            when (genre) {
                GenreChange.Keep -> Unit
                GenreChange.Clear -> put("genre", JsonNull)
                is GenreChange.To -> put("genre", genre.genre)
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
}

class AddonsView(
    client: CoreClient,
    scope: CoroutineScope,
) : RecordView<AddonsSnapshot>(client, scope, "addons", NO_PARAMS, AddonsSnapshot.serializer())

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
    /** Send on the first focus move: the pick under the cursor never changes after. */
    fun freeze() {
        CoreLog.d("versions: the pick is frozen")
        post(command("freeze"))
    }
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
    ) {
        if (positionMs < 0 || durationMs <= 0) return
        post(
            command("progress") {
                put("positionMs", positionMs)
                put("durationMs", durationMs)
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
    ) = post(
        command("failOver") {
            put("error", error)
            if (attempt != null) put("attempt", attempt)
        },
    )

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
}

/** `name` is how the account's other devices will show this one. */
class PairingView(
    client: CoreClient,
    scope: CoroutineScope,
    server: String,
    name: String,
) : RecordView<PairingSnapshot>(
        client,
        scope,
        "pairing",
        JsonObject(mapOf("server" to JsonPrimitive(server), "name" to JsonPrimitive(name))),
        PairingSnapshot.serializer(),
    ) {
    fun confirm() = post(command("confirm"))

    fun reject() = post(command("reject"))
}
