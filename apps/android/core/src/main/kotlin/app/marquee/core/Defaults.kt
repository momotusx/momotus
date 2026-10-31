package app.marquee.core

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import app.marquee.core.model.DefaultAddons
import app.marquee.core.model.DefaultInstalled
import app.marquee.core.route.DefaultsMode
import app.marquee.core.session.CoreOps
import app.marquee.core.views.AddonsView
import app.marquee.core.views.ViewStatus
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive

// The recommended addons (plans/m13-default-addons.md 6 and 7): the step, the Home card and the
// start decision, wired to the core's ops. The words and the choice rules are DefaultsCopy's.

/** The step's state: the core's list, the switches the user touched, and one install at a time. */
class DefaultsStep(
    private val ops: CoreOps,
    private val scope: CoroutineScope,
) {
    var answer by mutableStateOf<DefaultAddons?>(null)
        private set
    var problem by mutableStateOf<String?>(null)
        private set
    var failed by mutableStateOf<String?>(null)
        private set
    var busy by mutableStateOf(false)
        private set

    /** Only the rows the user touched; the others follow [DefaultsCopy.initial]. */
    val switches = mutableStateMapOf<String, Boolean>()

    fun load(): Job {
        problem = null
        return scope.launch {
            try {
                answer = ops.defaultAddons()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                problem = ErrorCopy.words(e)
            }
        }
    }

    /**
     * One `installDefaults` with the switches [mode] shows; [then] runs once it is written. Null while
     * one runs (a second press sends nothing) or before the list came. A failed or lost answer leaves
     * the switches as they are: Continue again sends the same states, and the core writes a no-op as
     * nothing.
     */
    fun save(
        mode: DefaultsMode,
        then: () -> Unit,
    ): Job? {
        val list = answer?.addons ?: return null
        if (busy) return null
        busy = true
        failed = null
        return scope.launch {
            try {
                ops.installDefaults(DefaultsCopy.choices(list, mode, switches))
                then()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                failed = ErrorCopy.words(e)
            } finally {
                busy = false
            }
        }
    }
}

/** The Home card for a profile that misses a default: once per list version, never for kids. */
class DefaultsCard(
    private val ops: CoreOps,
    private val scope: CoroutineScope,
) {
    var offer by mutableStateOf<DefaultAddons?>(null)
        private set

    // The version answered in this run: Home may ask again before the answer's write lands.
    private var answered = 0

    /** Asks the core whether to offer the card; [seen] reads the profile's `defaultsSeen` (null: no answer). */
    suspend fun check(
        kids: Boolean,
        seen: suspend () -> Int?,
    ) {
        val answer =
            try {
                ops.defaultAddons()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                return
            }
        if (answer.version <= answered || answer.addons.none { it.installed == DefaultInstalled.Absent }) {
            offer = null
            return
        }
        val version = seen() ?: return
        offer = answer.takeIf { DefaultsCopy.offer(it, version, kids) }
    }

    /** Review or Not now: the profile has seen this version (synced), and the card goes. */
    fun answer(): Job? {
        val version = offer?.version ?: return null
        answered = version
        offer = null
        return scope.launch {
            try {
                ops.setPrefs(JsonObject(mapOf("defaultsSeen" to JsonPrimitive(version))))
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                // ponytail: not retried; the card comes back on a later run, as if it was never answered.
            }
        }
    }

    /** A new profile: its own card, asked again. */
    fun forget() {
        offer = null
        answered = 0
    }
}

/**
 * Whether the profile starts at onboarding: the addons view's first answer says `fresh` (no list
 * ever, no sync account, not a kids profile). An empty list alone is not new: a person may have
 * removed every addon. The view is closed after.
 */
suspend fun startsFresh(addons: AddonsView): Boolean =
    try {
        addons.opened.await()
        snapshotFlow { addons.status }.first { it !is ViewStatus.Opening }
        addons.snapshot?.fresh == true
    } finally {
        addons.close()
    }
