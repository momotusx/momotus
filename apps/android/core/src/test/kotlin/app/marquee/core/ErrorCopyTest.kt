package app.marquee.core

import app.marquee.core.client.CoreError
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import java.io.File
import kotlin.test.Test
import kotlin.test.assertEquals

class ErrorCopyTest {
    @Test
    fun `the words are the shared ones, every key and no more`() {
        val shared =
            Json
                .parseToJsonElement(File("../../shared/error-words.json").readText())
                .jsonObject
                .filterKeys { it != "about" }
                .mapValues { it.value.jsonPrimitive.content }
        assertEquals(shared, ErrorCopy.byKind)
    }

    // The owner's rule: a failure shows its kind's words, never the core's raw text or a status.
    @Test
    fun `a failure shows its kind's words, never its raw text`() {
        val raw = "network: error sending request for url (http://127.0.0.1:9/manifest.json): Connection refused"
        assertEquals(ErrorCopy.NETWORK, ErrorCopy.words(CoreError(raw, kind = "network")))
        assertEquals(ErrorCopy.NOT_FOUND, ErrorCopy.words(CoreError("http status 404", kind = "notFound")))
        assertEquals(ErrorCopy.FAILED, ErrorCopy.words(CoreError("bad request: missing field `url`", kind = "failed")))
        // No kind (an older core, a request that timed out here, an app exception) is `failed`.
        assertEquals(ErrorCopy.FAILED, ErrorCopy.words(CoreError("internal error: task 7 panicked")))
        assertEquals(ErrorCopy.FAILED, ErrorCopy.words(IllegalStateException("boom")))
        assertEquals(ErrorCopy.FAILED, ErrorCopy.words(CoreError("x", kind = "somethingNew")))
        // The core's own sentence is kept.
        assertEquals(
            "Kids profiles never run plugins.",
            ErrorCopy.words(CoreError("Kids profiles never run plugins.", kind = "said")),
        )
    }
}
