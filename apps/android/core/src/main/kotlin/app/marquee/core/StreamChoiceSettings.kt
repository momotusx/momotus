package app.marquee.core

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import app.marquee.core.client.CoreError
import app.marquee.core.client.CoreLog
import app.marquee.core.model.StreamConditions
import app.marquee.core.session.CoreOps
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive

/**
 * Settings > Stream choice (plans/m13-stream-choice.md 4.4), as the web's page: each row writes one
 * `streams` field, Undo and Forget clear what was learned, and "This device" reads the core's
 * conditions, with "Measure again" once a speed was measured. The preferences themselves come from
 * the settings view, which follows every write. Main thread only.
 */
class StreamChoiceSettings(
    private val ops: CoreOps,
    private val scope: CoroutineScope,
) {
    /** What the core knows of this device's connection and disk; null until it answers. */
    var conditions by mutableStateOf<StreamConditions?>(null)
        private set

    /** "Measure again" was pressed: the speeds come back at the next playback. */
    var measuring by mutableStateOf(false)
        private set

    /** Why the last write did not work, in the core's words; null after one that did. */
    var problem by mutableStateOf<String?>(null)
        private set

    /** The first run's answer is being written: a second press sends nothing. */
    var answering by mutableStateOf(false)
        private set

    fun load() {
        scope.launch { conditions = runCatching { ops.streamConditions() }.getOrNull() }
    }

    /** A speed was measured here: "Measure again" has something to clear. It stays once pressed, so focus does too. */
    val measureShown: Boolean
        get() = measuring || conditions?.let { it.http.samples > 0 || it.p2p.samples > 0 } == true

    /** Writes one stream preference: `{"streams": {field: value}}`. */
    fun set(
        field: String,
        value: Any?,
    ) = write {
        ops.setPrefs(JsonObject(mapOf("streams" to JsonObject(mapOf(field to json(value))))))
    }

    /** The first run's question (4.5): the answer's two fields in one write, then [then]. */
    fun answer(
        a: StreamChoiceCopy.Answer,
        then: () -> Unit,
    ) {
        if (answering) return
        answering = true
        write(done = { answering = false }) {
            val streams = mapOf("sizeLimitBytes" to json(a.sizeLimitBytes), "minSeeders" to json(a.minSeeders))
            ops.setPrefs(JsonObject(mapOf("streams" to JsonObject(streams))))
            then()
        }
    }

    fun undo(row: StreamChoiceCopy.LearnedRow) = write { ops.undoLearned(row.screenClass, row.choice) }

    fun forget() = write { ops.forgetLearned() }

    fun measureAgain() {
        if (measuring) return
        write {
            val reset = ops.resetSpeed()
            // Measuring first: the button must never leave the page between the two.
            measuring = true
            conditions = reset
        }
    }

    private fun write(
        done: () -> Unit = {},
        op: suspend () -> Unit,
    ) {
        problem = null
        scope.launch {
            try {
                op()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                problem = (e as? CoreError)?.message ?: CoreLog.describe(e)
            } finally {
                done()
            }
        }
    }

    private fun json(value: Any?): JsonElement =
        when (value) {
            null -> JsonNull
            is Number -> JsonPrimitive(value)
            is Boolean -> JsonPrimitive(value)
            is String -> JsonPrimitive(value)
            is List<*> -> JsonArray(value.map(::json))
            else -> error("no JSON for $value")
        }

    companion object {
        /** A resolution chip's state: a cap at 4K or none is this screen's on every screen the apps run on. */
        fun resolutionOn(
            choice: Int?,
            maxHeight: Int?,
        ): Boolean = if (choice == null) maxHeight == null || maxHeight >= 2160 else maxHeight == choice
    }
}
