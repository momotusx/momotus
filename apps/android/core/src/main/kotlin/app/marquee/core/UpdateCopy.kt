package app.marquee.core

/** The update row's words (plans/auto-update.md section 5), the same on the web and Apple rows. */
object UpdateCopy {
    const val CHECK = "Check for updates"
    const val CHECKING = "Checking"
    const val CURRENT = "Up to date"
    const val FAILED = "Could not check. Try again later."
    const val INSTALL = "Install"
    const val INSTALL_NOW = "Install now"

    fun downloading(
        version: String,
        percent: Int?,
    ) = if (percent == null) "Downloading $version" else "Downloading $version, $percent%"

    /** No press needed: it goes in when the user leaves the app. */
    fun leaving(version: String) = "$version installs when you leave Momotus."

    /** A press is needed: the system asks, and the app closes. */
    fun ready(version: String) = "Version $version is ready. Momotus closes to install it."

    /** Home's quiet line while an update waits for a press. */
    fun home(version: String) = "Version $version is ready to install in Settings."
}
