package app.marquee.core.model

import kotlinx.serialization.KSerializer
import kotlinx.serialization.Serializable
import kotlinx.serialization.SerializationException
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.descriptors.SerialDescriptor
import kotlinx.serialization.encoding.Decoder
import kotlinx.serialization.encoding.Encoder
import kotlinx.serialization.json.JsonDecoder
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.int
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive

/**
 * Index-based list change from the core (mirrors `ListDiff` in mq-runtime and `diff.ts` in the
 * web family). Written by hand: it is the one generic type in the protocol.
 */
@Serializable(with = ListDiffSerializer::class)
sealed interface ListDiff<out T> {
    data class Append<out T>(
        val values: List<T>,
    ) : ListDiff<T>

    data object Clear : ListDiff<Nothing>

    data class PushFront<out T>(
        val value: T,
    ) : ListDiff<T>

    data class PushBack<out T>(
        val value: T,
    ) : ListDiff<T>

    data object PopFront : ListDiff<Nothing>

    data object PopBack : ListDiff<Nothing>

    data class Insert<out T>(
        val index: Int,
        val value: T,
    ) : ListDiff<T>

    data class Replace<out T>(
        val index: Int,
        val value: T,
    ) : ListDiff<T>

    data class Remove(
        val index: Int,
    ) : ListDiff<Nothing>

    data class Truncate(
        val length: Int,
    ) : ListDiff<Nothing>

    data class Reset<out T>(
        val values: List<T>,
    ) : ListDiff<T>
}

/** Applies one diff exactly like `ListDiff::apply` in the core. An out-of-range index throws. */
fun <T> MutableList<T>.applyDiff(diff: ListDiff<T>) {
    when (diff) {
        is ListDiff.Append -> {
            addAll(diff.values)
        }

        ListDiff.Clear -> {
            clear()
        }

        is ListDiff.PushFront -> {
            add(0, diff.value)
        }

        is ListDiff.PushBack -> {
            add(diff.value)
        }

        ListDiff.PopFront -> {
            if (isNotEmpty()) removeAt(0)
        }

        ListDiff.PopBack -> {
            if (isNotEmpty()) removeAt(lastIndex)
        }

        is ListDiff.Insert -> {
            add(diff.index, diff.value)
        }

        is ListDiff.Replace -> {
            this[diff.index] = diff.value
        }

        is ListDiff.Remove -> {
            removeAt(diff.index)
        }

        is ListDiff.Truncate -> {
            while (size > diff.length) removeAt(lastIndex)
        }

        is ListDiff.Reset -> {
            clear()
            addAll(diff.values)
        }
    }
}

fun <T> MutableList<T>.applyDiffs(diffs: List<ListDiff<T>>) {
    for (diff in diffs) applyDiff(diff)
}

class ListDiffSerializer<T>(
    private val element: KSerializer<T>,
) : KSerializer<ListDiff<T>> {
    private val values = ListSerializer(element)

    override val descriptor: SerialDescriptor =
        SerialDescriptor("app.marquee.core.model.ListDiff", JsonObject.serializer().descriptor)

    override fun deserialize(decoder: Decoder): ListDiff<T> {
        val input = decoder as JsonDecoder
        val obj = input.decodeJsonElement().jsonObject
        val json = input.json

        fun one(key: String): T = json.decodeFromJsonElement(element, obj.getValue(key))

        fun many(key: String): List<T> = json.decodeFromJsonElement(values, obj.getValue(key))

        fun index(key: String = "index"): Int = obj.getValue(key).jsonPrimitive.int
        return when (val op = obj.getValue("op").jsonPrimitive.content) {
            "append" -> ListDiff.Append(many("values"))
            "clear" -> ListDiff.Clear
            "pushFront" -> ListDiff.PushFront(one("value"))
            "pushBack" -> ListDiff.PushBack(one("value"))
            "popFront" -> ListDiff.PopFront
            "popBack" -> ListDiff.PopBack
            "insert" -> ListDiff.Insert(index(), one("value"))
            "set" -> ListDiff.Replace(index(), one("value"))
            "remove" -> ListDiff.Remove(index())
            "truncate" -> ListDiff.Truncate(index("length"))
            "reset" -> ListDiff.Reset(many("values"))
            else -> throw SerializationException("unknown list diff op: $op")
        }
    }

    override fun serialize(
        encoder: Encoder,
        value: ListDiff<T>,
    ): Unit = throw SerializationException("list diffs only travel from the core to the app")
}
