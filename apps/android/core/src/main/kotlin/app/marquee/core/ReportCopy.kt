package app.marquee.core

/**
 * Error reports' words (plans/error-reports.md 10), word for word `apps/shared/report-words.json`, which the
 * web and Apple read too; a test checks every key. The row sits in Settings, This device; the privacy page is
 * [PRIVACY_PATH] on the sync service's own host.
 */
object ReportCopy {
    const val LABEL = "Error reports"
    const val ON = "On"
    const val OFF = "Off"
    const val NOTE =
        "When Momotus crashes or something fails, it sends the Momotus team a short technical report: what failed, " +
            "the app and system version and the device model. Never your account, addons, what you watch or file " +
            "names, and we keep no network address. Turning this off deletes reports not sent yet."
    const val PRIVACY = "Privacy"
    const val PRIVACY_PATH = "/privacy"

    /** The words as the shared file keys them. */
    val byKey: Map<String, String> =
        mapOf(
            "label" to LABEL,
            "on" to ON,
            "off" to OFF,
            "note" to NOTE,
            "privacy" to PRIVACY,
            "privacyPath" to PRIVACY_PATH,
        )

    /** The privacy page on [syncServer]'s host, or null in a build without one. */
    fun privacyUrl(syncServer: String): String? = syncServer.trimEnd('/').takeIf { it.isNotEmpty() }?.plus(PRIVACY_PATH)
}
