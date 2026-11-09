package app.marquee.core

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import java.io.File
import kotlin.test.Test
import kotlin.test.assertEquals

/** First run's words are apps/shared/first-run-words.json's, every key and no more (as AccountRulesTest). */
class FirstRunCopyTest {
    @Test
    fun `the words are the shared ones, every key and no more`() {
        val shared = Json.parseToJsonElement(File("../../shared/first-run-words.json").readText()).jsonObject
        val theirs = shared.filterKeys { it != "about" }
        assertEquals(theirs.keys, FirstRunCopy.all.keys)
        for ((key, value) in theirs) {
            val ours = FirstRunCopy.all.getValue(key)
            if (value is JsonArray) {
                assertEquals(value.map { it.jsonPrimitive.content }, ours, key)
            } else {
                assertEquals(value.jsonPrimitive.content, ours, key)
            }
        }
    }
}
