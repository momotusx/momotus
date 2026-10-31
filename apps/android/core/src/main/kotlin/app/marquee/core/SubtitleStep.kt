package app.marquee.core

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import app.marquee.core.client.CoreError
import app.marquee.core.client.CoreLog
import app.marquee.core.model.LanguageChoice
import app.marquee.core.session.CoreOps
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive

/**
 * A first run's subtitles step, after the stream question (plans/m13-subtitle-languages.md 7): the language
 * subtitles start in, one choice. The [device]'s language, as the core lists it ("nb" reads as Norwegian, "no"),
 * is listed first and chosen, unless it is English (No subtitles then: most titles are in English, and English
 * audio a file does not tag would get English subtitles over it); then the common ones and No subtitles; every
 * other language waits behind More languages. [save] writes the choice as the profile's `subtitleLanguages` (none for No subtitles); Skip
 * writes nothing. Main thread only.
 */
class SubtitleStep(
    private val ops: CoreOps,
    private val scope: CoroutineScope,
    private val device: String?,
) {
    /** Every language the core knows, by name; empty until it answers. */
    var all by mutableStateOf(emptyList<LanguageChoice>())
        private set

    /** The chosen language's code; null for No subtitles. The device's, once the core's list is in ([choose]). */
    var chosen by mutableStateOf<String?>(null)
        private set

    /** The viewer chose: kept over the device's language when the list comes later. */
    private var touched = false

    /** The core's list is in, or failed: Continue waits for it, since the device's language is unknown before. */
    var loaded by mutableStateOf(false)
        private set

    /** Why the list did not come, in the core's words: the step still goes on with the common languages. */
    var listProblem by mutableStateOf<String?>(null)
        private set

    /** The choice is being written: a second press sends nothing. */
    var busy by mutableStateOf(false)
        private set

    /** Why the write did not work, in the core's words. */
    var problem by mutableStateOf<String?>(null)
        private set

    fun load() {
        scope.launch {
            val list =
                try {
                    ops.languages(device)
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    listProblem = (e as? CoreError)?.message ?: CoreLog.describe(e)
                    emptyList()
                }
            // The device's language as the core lists it; English stays on No subtitles.
            if (!touched) chosen = list.firstOrNull { it.device }?.code?.takeIf { it != "en" }
            all = list
            loaded = true
        }
    }

    /** The viewer picks [code] (null for No subtitles). */
    fun choose(code: String?) {
        touched = true
        chosen = code
    }

    /** The device's language as the core lists it ("no" for a Norwegian "nb"), for its row's line. */
    val mine: String?
        get() = all.firstOrNull { it.device }?.code

    /** The device's language as the core lists it, then the common ones ([LanguagePicks.COMMON]). */
    val common: List<LanguageChoice>
        get() = LanguagePicks.search(all, "", emptyList(), device)

    /** Every other language, by name. */
    val rest: List<LanguageChoice>
        get() = common.map { it.code }.toSet().let { shown -> all.filter { it.code !in shown } }

    /** Writes the choice, then [then]. */
    fun save(then: () -> Unit) {
        if (busy || !loaded) return
        busy = true
        problem = null
        scope.launch {
            try {
                val list = JsonArray(listOfNotNull(chosen).map(::JsonPrimitive))
                ops.setPrefs(JsonObject(mapOf("subtitleLanguages" to list)))
                then()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                problem = (e as? CoreError)?.message ?: CoreLog.describe(e)
            } finally {
                busy = false
            }
        }
    }

    companion object {
        const val TITLE = "Subtitles"
        const val LEAD =
            "Pick the language subtitles start in. They turn on when a film's audio is in another language. " +
                "Change it later in Settings, under Subtitles and audio."
        const val DEVICE = "This device's language"
        const val NONE = "No subtitles"
        const val NONE_LINE = "Turn them on while watching"
        const val MORE = "More languages"
        const val FEWER = "Fewer languages"
        const val SKIP = "Skip"
        const val FAILED = "Could not save the language"
        const val LIST_FAILED = "Could not load the languages"
    }
}
