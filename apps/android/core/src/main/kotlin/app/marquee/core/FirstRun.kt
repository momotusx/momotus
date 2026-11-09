package app.marquee.core

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import app.marquee.core.route.DefaultsMode
import app.marquee.core.session.CoreOps
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive

/**
 * First run's order (plans/first-screens.md 2, 4.3): which step shows next, and the setup sheet's subtitle chips.
 * The web and Apple apps hold the same rules, and all three are tested against
 * apps/shared/fixtures/first-run-vectors.json.
 */
object FirstRun {
    enum class Step { Welcome, Login, Profile, Who, Setup, Home, None }

    /** What just happened: a (re)start, a press, an account made or joined. */
    enum class After { Start, GetStarted, AlreadyUse, NotNow, Created, Joined, ProfileDone }

    /** The inputs, every one the core's or the app's own (the vectors file's about says what each means). */
    data class State(
        val fresh: Boolean,
        val kidsSession: Boolean,
        val server: Boolean,
        val inAccount: Boolean,
        val profileStepDone: Boolean,
        val profiles: Int,
        val pin: Boolean,
        val locked: Boolean,
    )

    /**
     * The step after [after]. A joined account opens Who's watching? with two or more profiles or a PIN, else Home
     * (D2); a locked session meets the picker first; a kids session and a profile that is not fresh see no first run.
     */
    fun next(
        s: State,
        after: After,
    ): Step =
        when {
            s.kidsSession -> Step.None
            after == After.Joined -> if (s.profiles >= 2 || s.pin) Step.Who else Step.Home
            !s.fresh -> Step.None
            s.locked -> Step.Who
            after == After.Start && s.profileStepDone -> Step.Setup
            after == After.Start -> if (s.inAccount) Step.Profile else Step.Welcome
            after == After.GetStarted -> if (s.server) Step.Login else Step.Profile
            after == After.AlreadyUse -> Step.Login
            after == After.ProfileDone -> Step.Setup
            else -> Step.Profile
        }

    /** The chip for "No subtitles" and the last one, which opens every language. */
    const val NONE = "none"
    const val MORE = "more"

    /** Setup's "Subtitles start in" chips, in order, and the one chosen when the sheet opens. */
    data class Chips(
        val chips: List<String>,
        val selected: String,
    )

    /**
     * D11: the profile's already chosen language first, else [device] (the device's language as the core lists it;
     * null when unknown or without a subtitle name), then English, then No subtitles and More. Never `noneFirst`:
     * English on an English device is chosen.
     */
    fun chips(
        device: String?,
        chosen: List<String>,
    ): Chips {
        val first = chosen.firstOrNull()
        return Chips(listOfNotNull(first, device, "en").distinct() + NONE + MORE, first ?: device ?: "en")
    }
}

/**
 * Make your first profile's Continue (plans/first-screens.md 3.4): `finishProfileStep` with [name] empty unless typed
 * and the avatar empty unless a swatch was pressed, so an untouched Continue writes nothing. One at a time; a failure
 * keeps the form as it is. Main thread only.
 */
class FirstProfileStep(
    private val ops: CoreOps,
    private val scope: CoroutineScope,
) {
    var busy by mutableStateOf(false)
        private set

    /** Why the last Continue did not go through, in the core's words. */
    var problem by mutableStateOf<String?>(null)
        private set

    fun finish(
        name: String,
        avatar: String,
        then: () -> Unit,
    ): Job? {
        if (busy) return null
        busy = true
        problem = null
        return scope.launch {
            try {
                ops.finishProfileStep(name.trim(), avatar)
                then()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                problem = ErrorCopy.words(e)
            } finally {
                busy = false
            }
        }
    }
}

/**
 * The setup sheet's writes (plans/first-screens.md 3.5): Start watching sends one `setPrefs` (the shown subtitle chip
 * always, `streams` only for Best quality), then installs every default not turned off unless the Stremio library
 * comes first; Skip installs the defaults and writes no prefs (D8). A failure keeps every answer and names the part
 * that failed; one press makes one write. Main thread only.
 */
class FirstSetup(
    private val ops: CoreOps,
    private val scope: CoroutineScope,
) {
    enum class Part { Prefs, Sources }

    /** "Bring my Stremio library" chosen over "Start fresh". */
    var bring by mutableStateOf(false)

    /** Best quality chosen over Start fast. */
    var best by mutableStateOf(false)

    /** The chip pressed; null keeps the one [FirstRun.chips] chose. */
    var picked by mutableStateOf<String?>(null)

    var busy by mutableStateOf(false)
        private set

    /** The part that did not go through, until the next press. */
    var failed by mutableStateOf<Part?>(null)
        private set

    /** Start watching with [subtitle] shown chosen (a code or [FirstRun.NONE]); [then] says whether Import is next. */
    fun start(
        subtitle: String,
        then: (import: Boolean) -> Unit,
    ): Job? {
        val languages = if (subtitle == FirstRun.NONE) emptyList() else listOf(JsonPrimitive(subtitle))
        val patch =
            buildMap {
                put("subtitleLanguages", JsonArray(languages))
                if (best) put("streams", streams(StreamChoiceCopy.BEST))
            }
        val import = bring
        return write(Part.Prefs) {
            ops.setPrefs(JsonObject(patch))
            if (!import) install(DefaultsMode.FirstRun)
            then(import)
        }
    }

    /** Skip: the defaults go in (so first run ends), no prefs. */
    fun skip(then: () -> Unit): Job? =
        write(Part.Sources) {
            install(DefaultsMode.FirstRun)
            then()
        }

    /** Import's Done (or Skip import, with nothing imported): the defaults the import did not bring, then [then]. */
    fun afterImport(then: () -> Unit): Job? =
        write(Part.Sources) {
            install(DefaultsMode.AfterImport)
            then()
        }

    private var part = Part.Prefs

    private suspend fun install(mode: DefaultsMode) {
        part = Part.Sources
        ops.installDefaults(DefaultsCopy.choices(ops.defaultAddons().addons, mode, emptyMap()))
    }

    private fun write(
        first: Part,
        work: suspend () -> Unit,
    ): Job? {
        if (busy) return null
        busy = true
        failed = null
        part = first
        return scope.launch {
            try {
                work()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                failed = part
            } finally {
                busy = false
            }
        }
    }

    private fun streams(a: StreamChoiceCopy.Answer) =
        JsonObject(
            mapOf(
                "sizeLimitBytes" to (a.sizeLimitBytes?.let(::JsonPrimitive) ?: JsonNull),
                "minSeeders" to JsonPrimitive(a.minSeeders),
            ),
        )
}
