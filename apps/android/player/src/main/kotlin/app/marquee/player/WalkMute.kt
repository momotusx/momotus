package app.marquee.player

import android.content.Context
import android.content.Intent

/**
 * Debug builds only (the activity reads it for those): walks on the owner's devices never make sound
 * (the repository rules), as Apple's `-MQMute YES`. A launch with `--ez mute true` mutes every player of this
 * install until a launch says `false` or the walk deletes the app's data; the device's volume is not touched.
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
        if (intent?.hasExtra("mute") ==
            true
        ) {
            kept.edit().putBoolean("mute", intent.getBooleanExtra("mute", false)).apply()
        }
        on = kept.getBoolean("mute", false)
    }
}
