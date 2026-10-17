package app.marquee.core.model

import kotlinx.serialization.KSerializer
import kotlinx.serialization.Serializable
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.descriptors.SerialDescriptor
import kotlinx.serialization.encoding.Decoder
import kotlinx.serialization.encoding.Encoder
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonDecoder
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonEncoder

/** A two-element JSON array such as `["wrongTitle", 3]` (ts-rs writes Rust tuples this way). */
@Serializable(with = Tuple2Serializer::class)
data class Tuple2<A, B>(
    val first: A,
    val second: B,
)

class Tuple2Serializer<A, B>(
    private val first: KSerializer<A>,
    private val second: KSerializer<B>,
) : KSerializer<Tuple2<A, B>> {
    override val descriptor: SerialDescriptor =
        SerialDescriptor("app.marquee.core.model.Tuple2", ListSerializer(JsonElement.serializer()).descriptor)

    override fun deserialize(decoder: Decoder): Tuple2<A, B> {
        val input = decoder as JsonDecoder
        val array = input.decodeJsonElement() as? JsonArray
        require(array != null && array.size == 2) { "expected a two-element array" }
        return Tuple2(
            input.json.decodeFromJsonElement(first, array[0]),
            input.json.decodeFromJsonElement(second, array[1]),
        )
    }

    override fun serialize(
        encoder: Encoder,
        value: Tuple2<A, B>,
    ) {
        val output = encoder as JsonEncoder
        output.encodeJsonElement(
            JsonArray(
                listOf(
                    output.json.encodeToJsonElement(first, value.first),
                    output.json.encodeToJsonElement(second, value.second),
                ),
            ),
        )
    }
}
