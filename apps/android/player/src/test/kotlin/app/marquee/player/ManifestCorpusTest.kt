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
            assertEverythingRead(file, playlist)
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

    /** Media3 read every variant, segment and part the file names: none was skipped. */
    private fun assertEverythingRead(
        file: File,
        playlist: HlsPlaylist,
    ) {
        val lines = file.readLines().map { it.trim() }.filter { it.isNotEmpty() }
        when (playlist) {
            is HlsMultivariantPlaylist -> {
                // I-frame variants are variants to Media3.
                val variants =
                    lines.count {
                        it.startsWith("#EXT-X-STREAM-INF:") || it.startsWith("#EXT-X-I-FRAME-STREAM-INF:")
                    }
                assertEquals(variants, playlist.variants.size, "${file.name}: variants")
            }

            is HlsMediaPlaylist -> {
                val segments = lines.count { !it.startsWith("#") }
                assertEquals(segments, playlist.segments.size, "${file.name}: segments")
                val parts =
                    lines.count {
                        it.startsWith("#EXT-X-PART:") || it.startsWith("#EXT-X-PRELOAD-HINT:TYPE=PART")
                    }
                val read = playlist.segments.sumOf { it.parts.size } + playlist.trailingParts.size
                assertEquals(parts, read, "${file.name}: parts")
            }
        }
    }

    private companion object {
        const val CORE = "http://127.0.0.1:7777/"
        const val CANARY = "http://canary.invalid/"
    }
}
