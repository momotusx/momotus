package app.marquee.android

import android.app.UiModeManager
import android.content.Context
import android.content.pm.PackageManager
import android.content.res.Configuration
import app.marquee.design.Form

/**
 * Which of the two UI trees this launch shows, decided once in `MainActivity.onCreate` and never
 * recomputed. The system says what the device is (the television UI mode, else the leanback
 * feature); a debug launch extra overrides it. The user's override in Settings arrives with the
 * Settings screen.
 */
object FormFactor {
    fun detect(
        context: Context,
        launchOverride: String?,
    ): Form {
        when (launchOverride) {
            "tv" -> return Form.Tv
            "phone" -> return Form.Phone
        }
        val mode = context.getSystemService(UiModeManager::class.java)?.currentModeType
        val television =
            mode == Configuration.UI_MODE_TYPE_TELEVISION ||
                context.packageManager.hasSystemFeature(PackageManager.FEATURE_LEANBACK)
        return if (television) Form.Tv else Form.Phone
    }
}
