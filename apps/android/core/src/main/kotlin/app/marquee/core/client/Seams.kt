package app.marquee.core.client

import app.marquee.core.model.Episode
import app.marquee.core.model.Network
import app.marquee.core.model.TitleDetails
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
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
            "previewAddon",
            "installAddon",
            "installListed",
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
            "traktSync",
            // A season or series reads the title's meta first.
            "downloadAdd",
        )

    fun forOp(op: String): Duration = if (op in networkOps) network else local
}

/**
 * What the device tells the ranking (`versions` params, plans/core-protocol.md). Names follow the
 * core's snake_case enums: decoders `h264`, `h265`, `av1`, `vp9`; audio decoders `aac`, `ac3`,
 * `eac3`, `dts`, `true_hd`, `opus`, `flac`, `mp3`, `pcm`; passthrough `ac3`, `eac3`, `dts`,
 * `dts_hd`, `true_hd`, `atmos`, `dts_x`; hdr `dolby_vision`, `hdr10`, `hdr10_plus`, `hlg`;
 * containers `mkv`, `mp4`, `webm`, `ts`, ... A release plays directly only when its video codec,
 * audio and container all fit; otherwise it needs the fallback engine. [language] is the device's
 * (ISO 639-1) and stands in for audio languages the user has not chosen. [network] is the default
 * network's type, for the speed the ranking reads (plans/m13-stream-choice.md 4.3). Left out means a
 * 1080p SDR stick.
 */
data class DeviceCapsParams(
    val displayHeight: Int,
    val hdr: List<String>,
    val decoders: List<String>? = null,
    val tenBitHevc: Boolean? = null,
    val passthrough: List<String>? = null,
    val audioDecoders: List<String>? = null,
    val containers: List<String>? = null,
    val language: String? = null,
    val hasFallbackEngine: Boolean? = null,
    val network: Network? = null,
) {
    fun toJson(): Map<String, JsonElement> =
        buildMap {
            put("displayHeight", JsonPrimitive(displayHeight))
            put("hdr", JsonArray(hdr.map(::JsonPrimitive)))
            decoders?.let { put("decoders", JsonArray(it.map(::JsonPrimitive))) }
            tenBitHevc?.let { put("tenBitHevc", JsonPrimitive(it)) }
            passthrough?.let { put("passthrough", JsonArray(it.map(::JsonPrimitive))) }
            audioDecoders?.let { put("audioDecoders", JsonArray(it.map(::JsonPrimitive))) }
            containers?.let { put("containers", JsonArray(it.map(::JsonPrimitive))) }
            language?.let { put("language", JsonPrimitive(it)) }
            hasFallbackEngine?.let { put("hasFallbackEngine", JsonPrimitive(it)) }
            network?.let { put("network", JsonPrimitive(it.wire)) }
        }
}

/**
 * The title as the app knows it, for the bitrate and the year checks (plans/m13-stream-choice.md 4.3):
 * the runtime ("142 min"), the release info ("2002", "2022-"), the episode's date ("2013-09-16") and
 * how many episodes its season has. The core reads the meta for what is left out.
 */
data class TitleFacts(
    val runtime: String? = null,
    val releaseInfo: String? = null,
    val released: String? = null,
    val episodesInSeason: Int? = null,
) {
    companion object {
        /** By video id: what a title page knows of the video Play starts and of each episode it lists. */
        fun of(
            details: TitleDetails?,
            playVideoId: String?,
            episodes: List<Episode>,
        ): Map<String, TitleFacts> {
            if (details == null) return emptyMap()
            val title = TitleFacts(details.runtime, details.releaseInfo)
            val inSeason = episodes.groupingBy { it.season }.eachCount()
            return buildMap {
                playVideoId?.let { put(it, title) }
                for (e in episodes) {
                    put(
                        e.videoId,
                        title.copy(released = e.released, episodesInSeason = inSeason[e.season]),
                    )
                }
            }
        }
    }
}

/** The `versions` subscription's params: the video, what the device plays, and what the app knows of the title. */
fun versionsParams(
    kind: String,
    videoId: String,
    title: String,
    caps: DeviceCapsParams,
    season: Int? = null,
    episode: Int? = null,
    bingeGroup: String? = null,
    facts: TitleFacts? = null,
    downloadFor: String? = null,
): JsonObject =
    JsonObject(
        buildMap {
            downloadFor?.let {
                put("purpose", JsonPrimitive("download"))
                put("itemId", JsonPrimitive(it))
            }
            put("kind", JsonPrimitive(kind))
            put("videoId", JsonPrimitive(videoId))
            put("title", JsonPrimitive(title))
            season?.let { put("season", JsonPrimitive(it)) }
            episode?.let { put("episode", JsonPrimitive(it)) }
            bingeGroup?.let { put("bingeGroup", JsonPrimitive(it)) }
            facts?.runtime?.let { put("runtime", JsonPrimitive(it)) }
            facts?.releaseInfo?.let { put("releaseInfo", JsonPrimitive(it)) }
            facts?.released?.let { put("released", JsonPrimitive(it)) }
            facts?.episodesInSeason?.let { put("episodesInSeason", JsonPrimitive(it)) }
            putAll(caps.toJson())
        },
    )

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
