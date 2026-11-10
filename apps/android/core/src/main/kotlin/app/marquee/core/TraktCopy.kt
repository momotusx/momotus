package app.marquee.core

import app.marquee.core.model.TraktImporting
import app.marquee.core.model.TraktLoginSnapshot
import app.marquee.core.model.TraktPlayback
import app.marquee.core.model.TraktReport
import app.marquee.core.model.TraktStatus

/**
 * Trakt copy (plans/m7-trakt.md section 3), word for word `apps/shared/trakt-words.json`, which the web
 * and Apple read too; a test checks every key. `{user}`, `{time}`, `{reason}` and the counts are filled
 * in by the functions below.
 */
object TraktCopy {
    const val TITLE = "Trakt"
    const val LEDE =
        "Trakt keeps a record of what you watch. With it connected, what you play here reaches Trakt, and " +
            "what you watched in other Trakt apps shows up here as watched and as places to resume. Each profile " +
            "connects its own account."
    const val ROW_OFF = "Keep watched marks and resume points in step with your Trakt account."
    const val CONNECTED = "Connected as {user}"
    const val CONNECTED_SHARED = "Connected for your account as {user}"
    const val CONNECTED_NO_USER = "Connected to Trakt"
    const val FAILING =
        "Trakt has not answered since {time}: {reason}. Momotus keeps trying, and nothing waiting here is " +
            "lost."
    const val RECONNECT = "Trakt needs you to sign in again ({reason}). Until then nothing is sent or brought in."
    const val OFF = "Not connected."
    const val THIS_DEVICE_OFF =
        "This profile uses Trakt on another device, but this device is not signed in, so what you play here " +
            "does not reach Trakt."
    const val LINKING = "Connecting this device to Trakt. It takes one sync."
    const val OFF_EVERYWHERE = "Trakt was disconnected on every device of your account."
    const val NOT_CONFIGURED = "Trakt is not set up in this build of Momotus, so nothing is sent to Trakt."
    const val CONNECT = "Connect Trakt"
    const val SIGN_IN_AGAIN = "Sign in again"
    const val SIGN_IN_HERE = "Sign in on this device"
    const val STARTING = "Asking Trakt for a code"
    const val OPEN = "On your phone or computer, open"
    const val SCAN_OR_OPEN = "Scan the code with your phone, or open"
    const val ENTER_CODE = "and enter this code:"
    const val GOOD_FOR = "Good for {time}"
    const val OPEN_PAGE = "Open the page"
    const val SAVING = "Saving the sign-in"
    const val EXPIRED = "The code ran out before it was entered. Get a new code to try again."
    const val DENIED = "The sign-in was turned down on Trakt, so nothing was connected."
    const val FAILED = "The sign-in did not finish: {reason}."
    const val NEW_CODE = "Get a new code"
    const val CANCEL = "Cancel"
    const val CLOSE = "Close"
    const val LAST_SYNCED = "Last synced {time}"
    const val NEVER_SYNCED = "Not synced with Trakt yet"
    const val PENDING_NONE = "Nothing waiting to go to Trakt"
    const val PENDING_ONE = "1 change waiting to go to Trakt"
    const val PENDING_MANY = "{count} changes waiting to go to Trakt"
    const val IMPORTING = "Bringing in your Trakt history: {done} of {total} shows"
    const val LAST_ERROR = "Last error: {error}"
    const val SYNC_NOW = "Sync now"
    const val SYNCING = "Syncing"
    const val SYNCED = "Synced: {pushed} sent, {applied} brought in"
    const val SYNC_FAILED = "Sync did not work: {reason}"
    const val SIGN_OUT = "Sign out"
    const val SIGN_OUT_ASK =
        "Sign out of Trakt on this device? Watched marks and progress stay here. Changes that have not " +
            "reached Trakt yet are dropped."
    const val SIGNED_OUT = "Signed out of Trakt on this device."
    const val SIGN_OUT_SHARED = "Disconnect on every device"
    const val SIGN_OUT_SHARED_ASK =
        "Disconnect Trakt on every device of your account? Watched marks and progress stay. Changes that have " +
            "not reached Trakt yet are dropped."
    const val SIGN_OUT_FAILED = "Not signed out: {reason}"
    const val MEMORY_ONLY =
        "Kept for this session only: this device could not store the sign-in. Sign in again after the app " +
            "restarts."
    const val UNMAPPED_TITLE = "Not matched"
    const val UNMAPPED_LEDE = "Trakt and Momotus could not agree on these, so they are not sent or brought in."
    const val PLAYER_ON = "Trakt: this play is recorded"
    const val PLAYER_UNMAPPED = "Trakt: this title could not be matched, so this play is not recorded"
    const val PLAYER_THIS_DEVICE_OFF = "Trakt: this device is not signed in, so this play is not recorded"
    const val PLAYER_RECONNECT = "Trakt: sign in again in Settings, so this play is not recorded"
    const val PLAYER_LINKING = "Trakt: this device has not received the sign-in yet, so this play is not recorded"

    /** Android only, not in the shared words: the core signed out, the file kept the sign-in. */
    const val SIGN_OUT_NOT_STORED =
        "Signed out of Trakt, but this device could not delete its copy of the sign-in, so it would come " +
            "back after a restart. Try again."
    const val TRY_AGAIN = "Try again"

    /** The code as a screen reader says it once: one character at a time. */
    fun spokenCode(code: String) = "Code ${code.toList().joinToString(" ")}"

    private fun String.fill(vararg pairs: Pair<String, Any>) =
        pairs.fold(this) { text, (key, value) -> text.replace("{$key}", value.toString()) }

    /** The state in words, the same line the Settings row and the page show. [time] formats a wall clock. */
    fun state(
        status: TraktStatus,
        time: (Long) -> String,
    ): String =
        when (status.state) {
            "connected" -> {
                status.user?.let { (if (status.shared) CONNECTED_SHARED else CONNECTED).fill("user" to it) }
                    ?: CONNECTED_NO_USER
            }

            "failing" -> {
                FAILING.fill("time" to time(status.sinceMs ?: 0), "reason" to status.reason.orEmpty())
            }

            "reconnect" -> {
                RECONNECT.fill("reason" to status.reason.orEmpty())
            }

            "thisDeviceOff" -> {
                THIS_DEVICE_OFF
            }

            "linking" -> {
                LINKING
            }

            "offEverywhere" -> {
                OFF_EVERYWHERE
            }

            "notConfigured" -> {
                NOT_CONFIGURED
            }

            // `off`, and a state this app does not know yet: nothing is promised.
            else -> {
                OFF
            }
        }

    /** The Settings row: what Trakt does while it is off, else the state. */
    fun row(
        status: TraktStatus?,
        time: (Long) -> String,
    ): String = if (status == null || status.state == "off") ROW_OFF else state(status, time)

    /** The sign-in each state offers, or none (connected, failing, a build without Trakt). */
    fun signIn(state: String): String? =
        when (state) {
            "off", "offEverywhere" -> CONNECT
            "reconnect" -> SIGN_IN_AGAIN
            "thisDeviceOff" -> SIGN_IN_HERE
            else -> null
        }

    /** In a Momotus account the connection is the account's: signing out disconnects every device. */
    fun signOut(shared: Boolean) = if (shared) SIGN_OUT_SHARED else SIGN_OUT

    fun signOutAsk(shared: Boolean) = if (shared) SIGN_OUT_SHARED_ASK else SIGN_OUT_ASK

    fun signedOut(shared: Boolean) = if (shared) OFF_EVERYWHERE else SIGNED_OUT

    /** A state the user can sync and sign out of: an account is held on this device. */
    fun signedIn(state: String) = state == "connected" || state == "failing" || state == "reconnect"

    fun pending(count: Int) =
        when (count) {
            0 -> PENDING_NONE
            1 -> PENDING_ONE
            else -> PENDING_MANY.fill("count" to count)
        }

    fun lastSynced(
        lastOkMs: Long?,
        time: (Long) -> String,
    ) = lastOkMs?.let { LAST_SYNCED.fill("time" to time(it)) } ?: NEVER_SYNCED

    fun importing(importing: TraktImporting) = IMPORTING.fill("done" to importing.done, "total" to importing.total)

    fun lastError(error: String) = LAST_ERROR.fill("error" to error)

    fun synced(report: TraktReport) = SYNCED.fill("pushed" to report.pushed, "applied" to report.applied)

    fun syncFailed(reason: String) = SYNC_FAILED.fill("reason" to reason)

    fun signOutFailed(reason: String) = SIGN_OUT_FAILED.fill("reason" to reason)

    fun goodFor(time: String) = GOOD_FOR.fill("time" to time)

    /** A sign-in that ended without a code to show; the code itself and `done` have their own layout. */
    fun login(phase: TraktLoginSnapshot): String =
        when (phase) {
            TraktLoginSnapshot.Starting -> STARTING
            TraktLoginSnapshot.Expired -> EXPIRED
            TraktLoginSnapshot.Denied -> DENIED
            TraktLoginSnapshot.NotConfigured -> NOT_CONFIGURED
            is TraktLoginSnapshot.Failed -> FAILED.fill("reason" to phase.reason)
            is TraktLoginSnapshot.Done -> SAVING
            is TraktLoginSnapshot.Code -> STARTING
        }

    /** What the player's Trakt mark says to a screen reader; null when the profile does not use Trakt (no mark). */
    fun player(trakt: TraktPlayback?): String? =
        when (trakt) {
            TraktPlayback.On -> PLAYER_ON
            TraktPlayback.Unmapped -> PLAYER_UNMAPPED
            TraktPlayback.ThisDeviceOff -> PLAYER_THIS_DEVICE_OFF
            TraktPlayback.Reconnect -> PLAYER_RECONNECT
            TraktPlayback.Linking -> PLAYER_LINKING
            TraktPlayback.Off, TraktPlayback.Unknown, null -> null
        }
}
