package app.marquee.core.client

import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonPrimitive
import kotlin.time.Duration
import kotlin.time.Duration.Companion.seconds

/**
 * How long a request may take. Requests that wait on the network (installs, sync, pairing) run
 * alongside the others in the core, so a long one no longer delays the rest; they just
 * legitimately take longer than a local request.
 */
object CoreTimeouts {
    val local: Duration = 8.seconds
    val network: Duration = 60.seconds
    private val networkOps =
        setOf(
            "installAddon",
            "syncNow",
            "approvePairing",
            "createRecoveryKit",
            "recover",
            "createSyncAccount",
            "syncDevices",
            "lostPreview",
            "removeDevice",
            "reportLost",
            "signOut",
            "useServerRoster",
        )

    fun forOp(op: String): Duration = if (op in networkOps) network else local
}

/**
 * What the device tells the ranking (`versions` params, plans/core-protocol.md). Names follow the
 * core's snake_case enums: decoders `h264`, `h265`, `av1`, `vp9`; passthrough `ac3`, `eac3`,
 * `dts`, `dts_hd`, `true_hd`, `atmos`, `dts_x`; hdr `dolby_vision`, `hdr10`, `hdr10_plus`, `hlg`.
 * Left out means a 1080p SDR stick.
 */
data class DeviceCapsParams(
    val displayHeight: Int,
    val hdr: List<String>,
    val decoders: List<String>? = null,
    val tenBitHevc: Boolean? = null,
    val passthrough: List<String>? = null,
    val hasFallbackEngine: Boolean? = null,
) {
    fun toJson(): Map<String, JsonElement> =
        buildMap {
            put("displayHeight", JsonPrimitive(displayHeight))
            put("hdr", JsonArray(hdr.map(::JsonPrimitive)))
            decoders?.let { put("decoders", JsonArray(it.map(::JsonPrimitive))) }
            tenBitHevc?.let { put("tenBitHevc", JsonPrimitive(it)) }
            passthrough?.let { put("passthrough", JsonArray(it.map(::JsonPrimitive))) }
            hasFallbackEngine?.let { put("hasFallbackEngine", JsonPrimitive(it)) }
        }
}

/** Where the app's logging goes; the app points it at logcat, tests leave it silent. */
object CoreLog {
    @Volatile
    var sink: (level: Char, message: String, error: Throwable?) -> Unit = { _, _, _ -> }

    fun d(message: String) = sink('d', message, null)

    fun w(
        message: String,
        error: Throwable? = null,
    ) = sink('w', message, error)

    fun e(
        message: String,
        error: Throwable? = null,
    ) = sink('e', message, error)

    /**
     * An error for a log line without the input that caused it: kotlinx.serialization quotes a
     * snippet of the JSON it choked on, which could hold a token or a key.
     */
    fun describe(error: Throwable): String =
        "${error.javaClass.simpleName}: ${error.message.orEmpty().substringBefore("JSON input").take(200)}"
}
