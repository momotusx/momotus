package app.marquee.core

import app.marquee.core.model.Blocked
import app.marquee.core.model.SyncReport
import kotlin.math.abs

/** The words of sync, equal to Apple's `SyncWords`: what went wrong and what a round left to say. */
object SyncCopy {
    /** A sync failure in plain words; [code] names the ones with a fix. */
    fun problem(
        code: String?,
        message: String,
    ): String {
        // The core's transport errors read "network: ..." with an address; nobody needs that.
        if (code == null && message.startsWith("network")) {
            return "The sync server did not answer. Your changes wait here and go out on the next sync."
        }
        return when (code) {
            "dropped" -> {
                "This device was removed from your sync account. Your library stays here as local data."
            }

            "fork" -> {
                "The sync server lists different devices than this device knows. " +
                    "Use the server's list to keep syncing."
            }

            "wrongRelay" -> {
                "This is not the sync server this device joined. " +
                    "Try again, or forget the account here and pair again."
            }

            "relayBehind" -> {
                "The sync server lost device entries it cannot get back. " +
                    "Pair this device again from one that still syncs."
            }

            "notJoined" -> {
                "This device has not joined a sync account yet."
            }

            "unsent" -> {
                "Some changes have not been sent yet. Sync again first, or leave anyway and they stay only here."
            }

            "insecure" -> {
                "The sync server address must start with https."
            }

            else -> {
                message
            }
        }
    }

    /** The keystore kept a new record in memory only: nothing is sent until it is stored. */
    const val STORAGE =
        "This device could not store the sync account's key, so sync is paused. Free some storage, " +
            "then sync again."

    /** A join whose record the keystore kept in memory only. */
    const val JOINED_SESSION_ONLY =
        "This device could not store the sync account's key: the join lasts until the app closes, " +
            "and sync is paused."

    /**
     * Why pairing stopped, in words. The core's own pairing reasons are fixed sentences and are kept;
     * a connection or address error is replaced, since its text can name the server's address.
     */
    fun pairingProblem(reason: String): String =
        when {
            reason.startsWith("pairing: ") -> {
                reason.removePrefix("pairing: ").replaceFirstChar { it.uppercase() } + "."
            }

            reason.startsWith("the relay must use https") -> {
                problem("insecure", reason)
            }

            reason.startsWith("invalid url") -> {
                "That is not a sync server address. It looks like https://sync.example.org."
            }

            reason.startsWith("blocked private address") -> {
                "This device does not connect to that address."
            }

            else -> {
                "The sync server did not answer. Check the address and try again."
            }
        }

    /** Things worth telling the user after a round, in words. */
    fun notes(report: SyncReport): List<String> =
        buildList {
            val gapMinutes = abs(report.clockGapMs) / 60_000
            if (abs(report.clockGapMs) > 600_000) {
                add(
                    "The sync server's clock and this device's disagree by $gapMinutes minutes, " +
                        "so edits sync in the order they are sent.",
                )
            }
            when (report.waiting) {
                0 -> Unit
                1 -> add("1 edit waits for the sync server's clock to catch up.")
                else -> add("${report.waiting} edits wait for the sync server's clock to catch up.")
            }
            when (report.parked) {
                0 -> Unit
                1 -> add("1 record waits for a key this device does not have yet.")
                else -> add("${report.parked} records wait for a key this device does not have yet.")
            }
            if (report.oldEpochWrites >
                0
            ) {
                add("The sync server may be hiding device changes. Compare the fingerprint on each device.")
            }
            if (report.rotationWaitMs > 0) {
                val minutes = maxOf(1, report.rotationWaitMs / 60_000)
                add("Changes wait $minutes ${if (minutes == 1L) "minute" else "minutes"} after a sync server restore.")
            }
            when (report.blocked) {
                Blocked.RotationPending -> {
                    add("Edits wait until a key change finishes.")
                }

                Blocked.NotActive -> {
                    add("This device is not active in the account, so its edits were not sent.")
                }

                Blocked.RelayDisagrees -> {
                    add(
                        "The sync server disagrees about the device list, so edits were not sent.",
                    )
                }

                null -> {
                    Unit
                }
            }
        }
}
