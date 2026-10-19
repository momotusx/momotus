package app.marquee.core.model

import kotlinx.serialization.KSerializer
import kotlinx.serialization.descriptors.PrimitiveKind
import kotlinx.serialization.descriptors.PrimitiveSerialDescriptor
import kotlinx.serialization.descriptors.SerialDescriptor
import kotlinx.serialization.encoding.Decoder
import kotlinx.serialization.encoding.Encoder

/**
 * An enum by its wire name, where a name this app does not know yet (a newer core added a value)
 * reads as [fallback] instead of failing the whole message: one new hide reason must not cost the
 * Versions list. The generated enums use it (scripts/dto.ts).
 */
open class FallbackEnumSerializer<E : Enum<E>>(
    serialName: String,
    private val values: List<E>,
    private val wire: (E) -> String,
    private val fallback: E,
) : KSerializer<E> {
    override val descriptor: SerialDescriptor = PrimitiveSerialDescriptor(serialName, PrimitiveKind.STRING)

    override fun deserialize(decoder: Decoder): E {
        val name = decoder.decodeString()
        return values.firstOrNull { wire(it) == name } ?: fallback
    }

    override fun serialize(
        encoder: Encoder,
        value: E,
    ) = encoder.encodeString(wire(value))
}
