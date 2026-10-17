package app.marquee.core

import app.marquee.core.client.CoreJson
import app.marquee.core.model.ListDiff
import app.marquee.core.model.applyDiff
import app.marquee.core.model.applyDiffs
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.builtins.serializer
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith

class ListDiffTest {
    @Test
    fun `matches the core semantics for every op`() {
        val list = mutableListOf<Int>()
        list.applyDiffs(
            listOf(
                ListDiff.PushBack(1),
                ListDiff.PushBack(2),
                ListDiff.Insert(1, 9),
                ListDiff.Replace(0, 5),
                ListDiff.Remove(2),
                ListDiff.PushFront(7),
                ListDiff.Append(listOf(8, 8)),
                ListDiff.PopBack,
                ListDiff.PopFront,
                ListDiff.Truncate(2),
            ),
        )
        assertEquals(listOf(5, 9), list)
        list.applyDiffs(listOf(ListDiff.Reset(listOf(1)), ListDiff.Clear))
        assertEquals(emptyList(), list)
    }

    @Test
    fun `popping an empty list is a no-op like in the core`() {
        val list = mutableListOf<Int>()
        list.applyDiff(ListDiff.PopFront)
        list.applyDiff(ListDiff.PopBack)
        assertEquals(emptyList(), list)
    }

    @Test
    fun `an index outside the list is an error the caller turns into a resubscribe`() {
        assertFailsWith<IndexOutOfBoundsException> { mutableListOf(1).applyDiff(ListDiff.Replace(3, 1)) }
        assertFailsWith<IndexOutOfBoundsException> { mutableListOf(1).applyDiff(ListDiff.Remove(2)) }
    }

    @Test
    fun `decodes every op as the core writes it`() {
        val json =
            """
            [{"op":"append","values":[1,2]},{"op":"clear"},{"op":"pushFront","value":3},{"op":"pushBack","value":4},
             {"op":"popFront"},{"op":"popBack"},{"op":"insert","index":1,"value":5},{"op":"set","index":0,"value":6},
             {"op":"remove","index":2},{"op":"truncate","length":3},{"op":"reset","values":[7]}]
            """.trimIndent()
        val diffs = CoreJson.decodeFromString(ListSerializer(ListDiffSerializerFor.int), json)
        assertEquals(11, diffs.size)
        assertEquals(ListDiff.Append(listOf(1, 2)), diffs[0])
        assertEquals(ListDiff.Replace(0, 6), diffs[7])
        assertEquals(ListDiff.Truncate(3), diffs[9])
    }
}

private object ListDiffSerializerFor {
    val int =
        app.marquee.core.model
            .ListDiffSerializer(Int.serializer())
}
