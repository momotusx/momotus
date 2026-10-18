package app.marquee.player

import android.net.Uri
import androidx.media3.common.util.UriUtil
import androidx.media3.exoplayer.hls.playlist.HlsMediaPlaylist
import androidx.media3.exoplayer.hls.playlist.HlsMultivariantPlaylist
import androidx.media3.exoplayer.hls.playlist.HlsPlaylistParser
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import java.io.File
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
        val dir = File("../../../crates/mq-server/tests/corpus/hls/out")
        val files =
            dir
                .listFiles()
                .orEmpty()
                .filter { it.name.endsWith(".m3u8") }
                .sortedBy { it.name }
        assertTrue(files.isNotEmpty(), "no corpus in ${dir.absolutePath}")
        for (file in files) {
            val base = Uri.parse("${CORE}s/token/${file.name}")
            val playlist = file.inputStream().use { HlsPlaylistParser().parse(base, it) }
            val keys = mutableListOf<String>()
            val urls =
                when (playlist) {
                    is HlsMultivariantPlaylist -> {
                        val renditions =
                            playlist.videos + playlist.audios + playlist.subtitles + playlist.closedCaptions
                        playlist.variants.map { it.url.toString() } +
                            renditions.mapNotNull { it.url?.toString() } +
                            playlist.mediaPlaylistUrls.map { it.toString() }
                    }

                    is HlsMediaPlaylist -> {
                        val segments =
                            playlist.segments.flatMap {
                                listOfNotNull(it, it.initializationSegment) +
                                    it.parts
                            }
                        keys += segments.mapNotNull { it.fullSegmentEncryptionKeyUri }
                        segments.map { it.url } +
                            playlist.trailingParts.map { it.url } +
                            playlist.renditionReports.keys.map { it.toString() }
                    }

                    else -> {
                        error("${file.name}: ${playlist.javaClass}")
                    }
                }
            assertTrue(urls.isNotEmpty(), "${file.name}: no URL read")
            for (url in urls + keys) {
                val resolved = UriUtil.resolve(base.toString(), url)
                val inline = url in keys && resolved.startsWith("data:")
                assertTrue(resolved.startsWith(CORE) || inline, "${file.name}: $resolved")
            }
        }
    }

    private companion object {
        const val CORE = "http://127.0.0.1:7777/"
    }
}
