package app.marquee.player

import android.content.Context
import android.content.Intent

/**
 * Debug builds only (the activity reads it for those): walks on the owner's devices never make sound
 * (the repository rules), as Apple's `-MQMute YES`. A launch with `--ez mute true` mutes every player of this
 * install until a launch says `false`, the app is opened from the launcher, or the walk deletes the app's
 * data; the device's volume is not touched. The launcher clears it because the owner opens the same walk
 * build from the icon afterwards and must hear it (2026-10-21: a kept mute played a film without sound).
 */
object WalkMute {
    @Volatile
    var on = false
        private set

    fun read(
        context: Context,
        intent: Intent?,
    ) {
        val kept = context.getSharedPreferences("walk", Context.MODE_PRIVATE)
        when {
            intent?.hasExtra("mute") == true -> {
                kept.edit().putBoolean("mute", intent.getBooleanExtra("mute", false)).apply()
            }

            intent?.action == Intent.ACTION_MAIN && intent.hasCategory(Intent.CATEGORY_LAUNCHER) -> {
                kept.edit().remove("mute").apply()
            }
        }
        on = kept.getBoolean("mute", false)
    }
}
