package app.marquee.player

import android.net.Uri
import androidx.media3.common.util.UriUtil
import androidx.media3.exoplayer.hls.playlist.HlsMediaPlaylist
import androidx.media3.exoplayer.hls.playlist.HlsMultivariantPlaylist
import androidx.media3.exoplayer.hls.playlist.HlsPlaylist
import androidx.media3.exoplayer.hls.playlist.HlsPlaylistParser
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import java.io.File
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * The core's rewritten playlists, read by the Media3 the app ships. The corpus is written by
 * `corpus_matches_the_rewriter` in crates/mq-server/src/manifest.rs (benign layouts and every
 * hostile input the rewrite keeps), for a core at [CORE]. Every URL Media3 exposes must be the
 * core's: a playlist that points anywhere else would let the player fetch from an origin.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class ManifestCorpusTest {
    @Test
    fun `every URL Media3 reads from a rewritten playlist is the core's`() {
        val files = corpus("out")
        for (file in files) {
            val base = "${CORE}s/token/${file.name}"
            val playlist = parse(file, base)
            val read = exposed(playlist)
            assertTrue(read.isNotEmpty(), "${file.name}: no URL read")
            for ((url, key) in read) {
                val resolved = UriUtil.resolve(base, url)
                val inline = key && resolved.startsWith("data:")
                assertTrue(resolved.startsWith(CORE) || inline, "${file.name}: $resolved")
            }
            // Media3 reads as much from the rewrite as from the origin's playlist.
            val input = File(file.parentFile?.parentFile, "in/${file.name}")
            val origin = parse(input, "https://cdn.example/hls/${file.name}")
            assertEquals(shape(origin), shape(playlist), "${file.name}: read from the input, then the output")
        }
    }

    /**
     * The controls the core refuses (`controls_are_refused` in manifest.rs): Media3 reads the
     * canary from each, so the refusals still matter. A Media3 that stops doing so fails here.
     */
    @Test
    fun `Media3 reads the canary from every control the core refuses`() {
        for (file in corpus("control")) {
            val base = "https://cdn.example/hls/${file.name}"
            val read = exposed(parse(file, base)).map { UriUtil.resolve(base, it.first) }
            assertTrue(read.any { it.startsWith(CANARY) }, "${file.name}: $read")
        }
    }

    private fun corpus(sub: String): List<File> {
        val dir = File("../../../crates/mq-server/tests/corpus/hls/$sub")
        val files =
            dir
                .listFiles()
                .orEmpty()
                .filter { it.name.endsWith(".m3u8") }
                .sortedBy { it.name }
        assertTrue(files.isNotEmpty(), "no corpus in ${dir.absolutePath}")
        return files
    }

    private fun parse(
        file: File,
        base: String,
    ): HlsPlaylist = file.inputStream().use { HlsPlaylistParser().parse(Uri.parse(base), it) }

    /** Every URL Media3 may fetch from [playlist], each with whether it names a key. */
    private fun exposed(playlist: HlsPlaylist): List<Pair<String, Boolean>> =
        when (playlist) {
            is HlsMultivariantPlaylist -> {
                val renditions =
                    playlist.videos + playlist.audios + playlist.subtitles + playlist.closedCaptions
                (
                    playlist.variants.map { it.url.toString() } +
                        renditions.mapNotNull { it.url?.toString() } +
                        playlist.mediaPlaylistUrls.map { it.toString() } +
                        listOfNotNull(playlist.contentSteeringInfo?.serverUri?.toString())
                ).map { it to false }
            }

            is HlsMediaPlaylist -> {
                val segments = playlist.segments + listOfNotNull(playlist.lastSeenInitSegment)
                val pieces = segments.flatMap { listOf(it) + it.parts } + playlist.trailingParts
                val withInits = pieces + pieces.mapNotNull { it.initializationSegment }
                val reports =
                    playlist.renditionReports.flatMap { (uri, report) ->
                        listOf(uri.toString(), report.playlistUri.toString())
                    }
                val interstitials =
                    playlist.interstitials.flatMap {
                        listOfNotNull(it.assetUri?.toString(), it.assetListUri?.toString())
                    }
                withInits.map { it.url to false } +
                    withInits.mapNotNull { it.fullSegmentEncryptionKeyUri }.map { it to true } +
                    (reports + interstitials).map { it to false }
            }

            else -> {
                error("${playlist.javaClass}")
            }
        }

    /**
     * How many of each thing Media3 read: variants and renditions, or segments, parts, trailing
     * parts, the init sections and keys of each of those three, and rendition reports. A rewrite
     * that makes Media3 skip one, or drop a key or init section anywhere, changes it.
     */
    private fun shape(playlist: HlsPlaylist): List<Int> =
        when (playlist) {
            is HlsMultivariantPlaylist -> {
                listOf(
                    playlist.variants.size,
                    playlist.videos.size,
                    playlist.audios.size,
                    playlist.subtitles.size,
                    playlist.closedCaptions.size,
                )
            }

            is HlsMediaPlaylist -> {
                val segments = playlist.segments
                val parts = segments.flatMap { it.parts }
                val trailing = playlist.trailingParts
                listOf(
                    segments.size,
                    parts.size,
                    trailing.size,
                    segments.count { it.initializationSegment != null },
                    segments.count { it.fullSegmentEncryptionKeyUri != null },
                    parts.count { it.initializationSegment != null },
                    parts.count { it.fullSegmentEncryptionKeyUri != null },
                    trailing.count { it.initializationSegment != null },
                    trailing.count { it.fullSegmentEncryptionKeyUri != null },
                    playlist.renditionReports.size,
                )
            }

            else -> {
                error("${playlist.javaClass}")
            }
        }

    private companion object {
        const val CORE = "http://127.0.0.1:7777/"
        const val CANARY = "http://canary.invalid/"
    }
}
