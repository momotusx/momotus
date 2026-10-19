package app.marquee.core

import app.marquee.core.model.Blocked
import app.marquee.core.model.SyncReport
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import java.util.TimeZone
import kotlin.math.abs
import kotlin.math.roundToLong

/**
 * Devices and sync copy, word for word the web's `syncWords` and Apple's `SyncWords` (where the
 * platforms differ, it says so). No protocol words: a device list, not a roster; "the 12 characters
 * to compare", not a fingerprint.
 */
object SyncCopy {
    const val TITLE = "Devices and sync"
    const val LEDE =
        "Sync keeps your library, progress, addons and settings the same on all your devices. The sync server " +
            "stores them sealed and cannot read them."
    const val OFF_LINE = "Keep your library, progress and addons the same on every device."
    const val PAIR_TITLE = "Pair this device"
    const val PAIR_TEXT = "Show a code here and approve it on a device that already syncs."
    const val START_TITLE = "Start a sync account"
    const val START_TEXT = "For your first device. Other devices join it with a code, so nothing is typed on them."
    const val RECOVER_TITLE = "Use a recovery kit"
    const val RECOVER_TEXT = "Join your account again when you have none of your devices."
    const val RECOVER_LEDE =
        "Joins your account as a new device. The kit is the 32 letters and digits you saved when you made it."
    const val SERVER = "Sync server"
    const val SERVER_HINT = "https://sync.example.org"
    const val DEVICE = "This device"
    const val DEVICE_HELP = "Your other devices list it by this name."
    const val INVITE = "Invite code"
    const val INVITE_HINT = "Only if your sync server asks for one"
    const val KIT = "Recovery kit"
    const val KIT_HINT = "XXXX-XXXX-XXXX-XXXX-XXXX-XXXX-XXXX-XXXX"
    const val BAD_ADDRESS = "Enter the address without a user name or password."
    const val NO_KIT_ENTERED = "Enter the recovery kit."
    const val START_BUTTON = "Start the account"
    const val STARTING = "Starting the account"
    const val RECOVER_BUTTON = "Join with the kit"
    const val RECOVERING = "Joining"
    const val WRONG_KIT = "The sync server does not know this kit. Check the letters and try again."
    const val REPLACE_ASK = "This device already syncs with an account"
    const val REPLACE_TEXT =
        "Joining another account replaces it here. Your library stays. Your other account still lists this device " +
            "until you remove it there."
    const val REPLACE_BUTTON = "Join the other account"
    const val SYNC_NOW = "Sync now"
    const val SYNCING = "Syncing"
    const val NOT_SYNCED = "Not synced yet"
    const val COMPARE_LABEL = "Compare on each device"
    const val COMPARE_HELP =
        "Once they have synced, all your devices show these same 12 characters. If one shows different ones, the " +
            "sync server may be hiding a device change."
    const val DEVICES = "Devices"
    const val LOADING_DEVICES = "Asking the sync server for your devices"
    const val THIS_DEVICE = "This device"
    const val UNNAMED = "Unnamed device"
    const val UNKNOWN = "Unknown device"
    const val UNKNOWN_DETAIL = "Not in this device's list yet. It goes with the others."
    const val KIT_NAME = "Recovery kit"
    const val REMOVE = "Remove"
    const val REPORT_LOST = "Report lost"
    const val CANCEL = "Cancel"
    const val ADD_TITLE = "Add a device"
    const val ADD_TEXT = "Approve a new device with the code it shows."
    const val KIT_TITLE = "Make a recovery kit"
    const val KIT_TEXT = "Get your account back if you lose all your devices."
    const val NO_KIT =
        "This account has no recovery kit. If you lose all your devices, your synced data cannot be read again. " +
            "Make one and keep it somewhere private."
    const val AFTER_RECOVER = "You joined with a recovery kit. Make a new one now, and remove the device you lost."
    const val TV_ONLY = "On your phone or computer"
    const val TV_ADD = "Add devices from your phone or computer: a TV has no camera to scan the code."
    const val TV_KIT = "Make recovery kits on your phone or computer: on a TV, anyone in the room can read one."
    const val TV_AFTER_RECOVER =
        "You joined with a recovery kit. Make a new one on your phone or computer, and remove the device you lost."
    const val LEAVE_TITLE = "Stop syncing on this device"
    const val LEAVE_TEXT = "Your library stays here. Edits not sent yet go out first."
    const val LEAVE_BUTTON = "Stop syncing"
    const val LEAVE_ANYWAY = "Stop anyway"
    const val SYNC_AGAIN = "Sync again"
    const val LEFT = "This device no longer syncs. Your library stays here."
    const val USE_SERVER = "Use the server's device list"
    const val FORGET_HERE = "Forget it on this device"
    const val FORGET_HERE_TEXT =
        "Forgetting needs no sync server and keeps your library. Your other devices still list this one until " +
            "you remove it there."

    // Add a device
    const val ADD_LEDE =
        "On the new device, open Settings, then Devices and sync, then Pair this device. It shows a code."
    const val ADD_WARNING =
        "Approve only a code on a screen in front of you. Whoever controls that screen gets your account."
    const val SCAN = "Scan the code"
    const val SCANNING = "Point the camera at the code on the other device."
    const val NO_CAMERA = "The camera is not available. Paste the code instead."
    const val PASTE_LABEL = "Or paste the code"
    const val PASTE_ONLY = "Paste the code"
    const val PASTE_HINT = "marquee:pair?..."
    const val NOT_A_CODE = "That is not a pairing code. It starts with marquee:pair."
    const val APPROVE = "Approve"
    const val APPROVING = "Approving"
    const val APPROVED_STEP = "The new device now shows six digits. They must be these:"
    const val APPROVED_HELP =
        "If they match, confirm on the new device. If they differ, say so there, and nothing is added."
    const val DONE = "Done"

    // The joining device's side
    const val COPY_CODE = "Copy the code"
    const val COPIED = "Copied"
    const val COPY_HELP = "No camera? Copy the code, then paste it on the other device under Add a device."

    // Recovery kit
    const val KIT_LEDE =
        "A recovery kit gets your account back when you have none of your devices. Anyone who has it can join your " +
            "account, so keep it somewhere private, like a password manager."
    const val KIT_ONCE = "Marquee shows the kit once and keeps no copy."
    const val MAKE_KIT = "Make the kit"
    const val MAKING_KIT = "Making the kit"
    const val KIT_SHOWN = "Your recovery kit"
    const val KIT_WARNING = "Save it now. It is not shown again, and anyone who has it can join your account."
    const val COPY_KIT = "Copy"

    /** Android: the clipboard has no expiry, so the app clears it, and marks the kit private for previews. */
    const val COPY_KIT_HELP =
        "Copying puts it on this device's clipboard, marked private, and Marquee empties the clipboard when you " +
            "leave this page."
    const val KIT_COPIED =
        "The kit is on your clipboard. Paste it into your password manager now: Marquee empties the clipboard " +
            "when you leave this page."
    const val SAVED_KIT = "I saved it"

    // Remove and report lost
    const val REMOVE_KIT_ASK = "Remove this recovery kit? It stops working."
    const val LOST_LEDE = "{name} is removed from your account and cannot read anything you change from now on."
    const val LOST_ADDED = "Devices added through {name}"
    const val LOST_ADDED_HELP =
        "Whoever has {name} may have these too, so they are removed as well. Untick the ones you still have."
    const val LOST_CHECKING = "Checking what {name} added"
    const val REMOVED = "Removed"
    const val RETRY = "Try again"

    /** A TV with no screen lock asks a second time instead of the system prompt. */
    const val TV_SURE = "Nobody can confirm it is you on this TV. Are you sure?"

    // Why this device asks who is there (the system prompt's subtitle)
    const val AUTH_REMOVE = "Remove a device from your sync account"
    const val AUTH_LOST = "Report a device lost"
    const val AUTH_KIT = "Make a recovery kit"

    /** The keystore kept a new record in memory only: nothing is sent until it is stored. */
    const val STORAGE =
        "This device could not store the sync account's key, so sync is paused. Free some storage, " +
            "then sync again."

    /** A join whose record the keystore kept in memory only. */
    const val JOINED_SESSION_ONLY =
        "This device could not store the sync account's key: the join lasts until the app closes, " +
            "and sync is paused."

    /** Fills `{name}` in a sync line. */
    fun named(
        line: String,
        name: String,
    ) = line.replace("{name}", name)

    /** "1 device" or "3 devices". */
    fun count(
        n: Long,
        one: String,
        many: String,
    ) = "$n ${if (n == 1L) one else many}"

    fun count(
        n: Int,
        one: String,
        many: String,
    ) = count(n.toLong(), one, many)

    /** The confirm line for removing another device. */
    fun removeDeviceAsk(name: String) =
        "Remove $name? It stops syncing and cannot read anything you change from now on. Devices it added stay."

    /** The step before approving a code: which sync server it joins through. */
    fun approveAsk(host: String) = "Approve a device from $host?"

    /** The button that confirms a lost report: it names how many devices go. */
    fun lostButton(devices: Int) = "Remove ${count(devices, "device", "devices")}"

    /** The kits a lost report removes, always. */
    fun lostKits(kits: Int) =
        (
            if (kits ==
                1
            ) {
                "1 recovery kit made on these devices stops"
            } else {
                "$kits recovery kits made on these devices stop"
            }
        ) +
            " working. Make a new one afterwards."

    /** A member's second line: when it was added, and through which device. */
    fun addedLine(
        kit: Boolean,
        addedMs: Long,
        by: String?,
        zone: TimeZone = TimeZone.getDefault(),
    ): String {
        val day = SimpleDateFormat("MMM d, yyyy", Locale.US).apply { timeZone = zone }.format(Date(addedMs))
        if (kit) return if (by != null) "Made $day on $by" else "Made $day"
        return if (by != null) "Added $day through $by" else "Added $day, the first device"
    }

    /** "Synced at 3:15 PM: 2 changes sent, 5 changes received." */
    fun syncedLine(
        at: String,
        report: SyncReport,
    ) = "Synced at $at: ${count(report.pushed, "change sent", "changes sent")}, " +
        "${count(report.applied, "change received", "changes received")}."

    /**
     * Where a sync failure happened: some answers mean something only there (a 403 when starting is an
     * invite, a 404 on a kit an unknown kit, a 404 on a code a used one).
     */
    enum class Action { Round, Start, Recover, Approve, Devices, Kit, Remove, Lost, Leave }

    /**
     * A sync failure in plain words; [code] names the ones with a fix. The core's own sentences are
     * kept; anything that may name the server's address is replaced.
     */
    fun problem(
        code: String?,
        message: String,
        action: Action = Action.Round,
    ): String {
        when (code) {
            "dropped" -> {
                return "This device was removed from your sync account. Your library stays here as local data."
            }

            "fork" -> {
                return "The sync server lists different devices than this device knows. Use the server's list to " +
                    "keep syncing."
            }

            "wrongRelay" -> {
                return "This is not the sync server this device joined. Try again, or forget the account here and " +
                    "pair again."
            }

            "relayBehind" -> {
                return "The sync server lost device changes it cannot get back. Pair this device again from one " +
                    "that still syncs."
            }

            "notJoined" -> {
                return "This device has not joined a sync account yet."
            }

            "unsent" -> {
                return "Some changes have not been sent yet. Sync again first, or stop anyway and they stay only here."
            }

            "insecure" -> {
                return "The sync server address must start with https."
            }

            // The app's own codes.
            "storage" -> {
                return STORAGE
            }

            "unconfirmed" -> {
                return "Nothing changed: this device did not confirm it is you."
            }

            "noLock" -> {
                return "Set a screen lock first: Marquee asks for it before changing your devices or making a kit."
            }
        }
        if (action == Action.Start && message == "http status 403") {
            return "This sync server needs an invite code, or the one entered is wrong."
        }
        if (action == Action.Recover && message == "http status 404") return WRONG_KIT
        if (action == Action.Approve && (message == "http status 404" || message == "http status 410")) {
            return "This code expired or was used already. Ask the new device for a new one."
        }
        if (message.startsWith("roster: ")) return pairingProblem("pairing: " + message.removePrefix("roster: "))
        for (kept in listOf("pairing: ", "invalid url", "blocked private address", "the relay must use https")) {
            if (message.startsWith(kept)) return pairingProblem(message)
        }
        return when (action) {
            Action.Round -> "The sync server did not answer. Your changes wait here and go out on the next sync."
            Action.Devices -> "The sync server did not answer, so this list may be out of date. Try again in a moment."
            else -> "The sync server did not answer. Try again in a moment."
        }
    }

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

    /** What a round's facts mean for the user (plans/core-protocol.md, `syncNow`), one line each. */
    fun notes(report: SyncReport): List<String> =
        buildList {
            val gap = abs(report.clockGapMs)
            if (gap > 600_000) {
                val minutes = (gap / 60_000.0).roundToLong()
                add(
                    "The sync server's clock and this device's disagree by ${count(minutes, "minute", "minutes")}, " +
                        "so edits sync in the order they are sent.",
                )
            }
            if (report.waiting > 0) {
                val edits = if (report.waiting == 1) "1 edit waits" else "${report.waiting} edits wait"
                add("$edits for the sync server's clock to catch up. Nothing is lost.")
            }
            if (report.parked > 0) {
                val changes =
                    if (report.parked == 1) {
                        "1 change from another device waits"
                    } else {
                        "${report.parked} changes from other devices wait"
                    }
                add("$changes until this device catches up with a device change.")
            }
            if (report.oldEpochWrites > 0) {
                add("The sync server may be hiding a device change. Compare the 12 characters below on each device.")
            }
            if (report.rotationWaitMs > 0) {
                val minutes = maxOf(1L, (report.rotationWaitMs + 59_999) / 60_000)
                add(
                    "Edits wait ${count(minutes, "minute", "minutes")} more: the sync server was restored, and other " +
                        "devices get a chance to catch up first.",
                )
            }
            if (report.refused > 0) {
                val n = report.refused
                add(
                    "${if (n == 1) "1 edit was" else "$n edits were"} too large or malformed for the sync server, so " +
                        "${if (n == 1) "it stays" else "they stay"} only on this device.",
                )
            }
            if (report.unreadable > 0) {
                val n = report.unreadable
                add(
                    "${if (n == 1) "1 change" else "$n changes"} from another device did not open with this " +
                        "account's key and ${if (n == 1) "was" else "were"} skipped.",
                )
            }
            if (report.newer > 0) {
                val n = report.newer
                val changes = if (n == 1) "1 change comes" else "$n changes come"
                add("$changes from a newer version of Marquee. Update the app to see ${if (n == 1) "it" else "them"}.")
            }
            when (report.blocked) {
                Blocked.RotationPending -> {
                    add("Edits wait until the account finishes a device change.")
                }

                Blocked.NotActive -> {
                    add("The sync server does not take edits from this device yet, so they were not sent.")
                }

                Blocked.RelayDisagrees -> {
                    add(
                        "The sync server disagrees about your device list, so edits were not sent. Compare the 12 " +
                            "characters below on each device.",
                    )
                }

                null -> {
                    Unit
                }
            }
        }
}
