package app.marquee.player

import android.content.Context
import android.content.Intent
import android.os.Handler
import android.os.StatFs
import android.os.SystemClock
import android.system.Os
import android.util.Log
import androidx.annotation.OptIn
import androidx.media3.common.C
import androidx.media3.common.Player
import androidx.media3.common.util.UnstableApi
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.analytics.PlaybackStatsListener
import org.json.JSONObject
import java.io.File

/**
 * Debug builds only (the activity reads it for those), as Apple's `-MQWalkLog YES`: after a launch with
 * `--ez walklog true` every player of this install writes one `walk {...}` line a second to logcat (tag
 * `walk`) while it plays or waits, the row Apple's walk log writes, so a phone walk reads like the Mac's
 * CLI rows:
 * - `stalls`, `stall_ms`: ExoPlayer's rebuffers after the first frame (a seek's wait is not one);
 * - `player_stalls`: the player's own count, here the same; `observed_bitrate` its mean bandwidth
 *   (bits/s) and `startup_ms` its join time, null while it has none;
 * - `free_bytes`: the free space of the app's data volume;
 * - `allocated_bytes`: what the core's torrent folder takes on disk (holes not counted), the CLI's
 *   `allocated_bytes`.
 */
@OptIn(UnstableApi::class)
class WalkLog private constructor(
    private val context: Context,
    private val player: ExoPlayer,
) {
    private val stats = PlaybackStatsListener(false, null)
    private val opened = SystemClock.elapsedRealtime()
    private val handler = Handler(player.applicationLooper)
    private val tick =
        object : Runnable {
            override fun run() {
                log()
                handler.postDelayed(this, 1_000)
            }
        }

    init {
        player.addAnalyticsListener(stats)
        handler.postDelayed(tick, 1_000)
    }

    fun release() = handler.removeCallbacks(tick)

    private fun log() {
        val waiting = player.playbackState == Player.STATE_BUFFERING && player.playWhenReady
        if (!player.isPlaying && !waiting) return
        val played = stats.combinedPlaybackStats
        val bandwidth = played.meanBandwidth
        val join = played.meanJoinTimeMs
        // Keys in Apple's order (sorted), so the two logs diff line for line.
        val row =
            JSONObject()
                .put("allocated_bytes", allocatedBytes())
                .put("free_bytes", StatFs(context.filesDir.path).availableBytes)
                .put("kind", "playback")
                .put("observed_bitrate", if (bandwidth == C.LENGTH_UNSET) JSONObject.NULL else bandwidth)
                .put("player_stalls", played.totalRebufferCount)
                .put("position_ms", player.currentPosition)
                .put("stall_ms", played.totalRebufferTimeMs)
                .put("stalls", played.totalRebufferCount)
                .put("startup_ms", if (join == C.TIME_UNSET) JSONObject.NULL else join)
                .put("t_ms", SystemClock.elapsedRealtime() - opened)
        Log.i("walk", "walk $row")
    }

    private fun allocatedBytes() =
        File(context.filesDir, "marquee/torrents")
            .walk()
            .filter { it.isFile }
            .sumOf { runCatching { Os.stat(it.path).st_blocks * 512 }.getOrDefault(0L) }

    companion object {
        @Volatile
        var on = false
            private set

        fun read(
            context: Context,
            intent: Intent?,
        ) {
            val kept = context.getSharedPreferences("walk", Context.MODE_PRIVATE)
            if (intent?.hasExtra("walklog") == true) {
                kept.edit().putBoolean("walklog", intent.getBooleanExtra("walklog", false)).apply()
            }
            on = kept.getBoolean("walklog", false)
        }

        /** The log of [player] while [on], released with it. */
        fun attach(
            context: Context,
            player: ExoPlayer,
        ) = if (on) WalkLog(context, player) else null
    }
}
