package app.marquee.core

import app.marquee.core.model.Blocked
import app.marquee.core.model.SyncReport
import java.text.DateFormat
import java.util.Date
import java.util.Locale
import java.util.TimeZone
import kotlin.math.abs
import kotlin.math.roundToLong

/**
 * Account (the sync page) copy, word for word the web's `syncWords` and Apple's `SyncWords` (where the
 * platforms differ, it says so). No protocol words: a device list, not a roster; "the 12 characters
 * to compare", not a fingerprint.
 */
object SyncCopy {
    const val TITLE = "Account"
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
    const val USE_SERVER = "Compare device lists"
    const val LEAVE_HERE = "Stop syncing here only"
    const val LEAVE_HERE_TEXT =
        "This device forgets the account without telling the sync server, which lists it until another device " +
            "removes it. Your library stays here, and you can pair this device again."

    /** Android asks first: the key goes, and with it the only way in when no other device or kit holds one. */
    const val LEAVE_HERE_ASK =
        "Stop syncing here only? This device deletes its key for the account. If no other device and no " +
            "recovery kit holds this account, its synced data cannot be read again."
    const val LEAVE_HERE_BUTTON = "Stop syncing here"
    const val LEFT_HERE =
        "This device no longer syncs. The sync server still lists it until another device removes it; you can " +
            "pair it again."

    // Delete the account
    const val DELETE_TITLE = "Delete account"
    const val DELETE_TEXT =
        "Deletes your account from the sync service for all your devices. Each device keeps its own copy of your " +
            "library."
    const val DELETE_BUTTON = "Delete account"
    const val DELETE_ASK =
        "Your profiles, library, progress and settings are deleted from the sync service. This device keeps its " +
            "own copy."
    const val DELETING = "Deleting"
    const val ACCOUNT_GONE = "Your Momotus account was deleted. Your library stays on this device."
    const val OK = "OK"

    // Add a device
    const val ADD_LEDE =
        "On the new device, open Settings, then Account, then Pair this device. It shows a code."
    const val ADD_WARNING =
        "Approve only a code on a screen in front of you. Whoever controls that screen gets your account."
    const val SCAN = "Scan the code"
    const val SCANNING = "Point the camera at the code on the other device."

    // Only a live scan approves (plans/accounts.md D6): no paste field, so no words for one.
    const val NO_CAMERA = "The camera is not available."
    const val NOT_A_CODE = "This code is not a Momotus pairing code."
    const val NEXT = "Next"
    const val CAMERA_DENIED = "Momotus may not use the camera. Allow it in the system settings."

    /** The step before approving: the code's server is always this account's own, so it asks about the device. */
    const val REVIEW_ASK = "Approve a new device?"
    const val APPROVE = "Approve"
    const val APPROVING = "Approving"
    const val APPROVED_STEP = "The new device now shows six digits. They must be these:"
    const val APPROVED_HELP =
        "If they match, confirm on the new device. If they differ, say so there, and press They differ here."
    const val DONE = "Done"
    const val DIFFER = "They differ"
    const val ADD_AGAIN = "Add a device again"
    const val SHOW_LETTERS = "Show the letters"
    const val HIDE_LETTERS = "Hide the letters"
    const val DIFFER_ENDED =
        "The approval ended and your sync key changed: whoever answered cannot join with it, and a device that " +
            "already did is removed, with any device it added."

    // Compare device lists, after the sync server showed a different one
    const val FORK_TITLE = "Compare device lists"
    const val FORK_LEDE =
        "The sync server lists different devices than this device knows. That happens when the server was " +
            "restored from a backup, or when someone changed it. Check its list, tick anything you do not " +
            "recognise, then keep syncing with it."
    const val FORK_LOADING = "Asking the sync server for its list"
    const val FORK_SERVER = "On the sync server's list"
    const val FORK_SERVER_HELP =
        "Ticked devices are removed when you continue, with the recovery kits they made. Those marked Goes are " +
            "removed anyway: this device had removed them, they are recovery kits only the server knows, or they " +
            "go with a ticked device."
    const val FORK_GOES = "Goes"
    const val FORK_SERVER_ONLY = "Only on the server's list"
    const val FORK_MINE = "Only this device knew these"
    const val FORK_MINE_HELP =
        "They are not on the server's list, so they are gone once you continue: a device among them must pair " +
            "again, and a recovery kit among them stops working. Make a new kit afterwards if one was yours."
    const val FORK_SAME = "Both lists name the same devices."
    const val FORK_GO = "Continue with the server's list"
    const val FORK_GOING = "Continuing"
    const val FORK_OFF_LIST =
        "The sync server's list no longer includes this device: it was removed there. Pair this device again " +
            "from one that still syncs."
    const val FORK_CHANGED =
        "The sync server's list changed while you looked. Here it is again; check it before you continue."
    const val FORK_TOKEN_LOST =
        "This device cannot read the sync server's list: the server no longer knows it. Sync from another of " +
            "your devices, or pair this one again."
    const val FORK_NONE = "This device and the sync server list the same devices. There is nothing to compare."

    // The joining device's side
    const val COPY_CODE = "Copy the code"
    const val COPIED = "Copied"
    const val COPY_HELP = "No camera? Copy the code, then paste it on the other device under Add a device."

    // Recovery kit
    const val KIT_LEDE =
        "A recovery kit gets your account back when you have none of your devices. Anyone who has it can join your " +
            "account, so keep it somewhere private, like a password manager."
    const val KIT_ONCE = "Momotus shows the kit once and keeps no copy."
    const val MAKE_KIT = "Make the kit"
    const val MAKING_KIT = "Making the kit"
    const val KIT_SHOWN = "Your recovery kit"
    const val KIT_WARNING = "Save it now. It is not shown again, and anyone who has it can join your account."
    const val COPY_KIT = "Copy"

    /** Android: the clipboard has no expiry, so the app clears it, and marks the kit private for previews. */
    const val COPY_KIT_HELP =
        "Copying puts it on this device's clipboard, marked private. Momotus empties the clipboard after a " +
            "minute, or when you leave this page."
    const val KIT_COPIED =
        "The kit is on your clipboard for a minute. Paste it into your password manager now: Momotus empties " +
            "the clipboard after that, or when you leave this page."
    const val SAVED_KIT = "I saved it"
    const val KIT_WAITING = "Your new recovery kit is not saved yet. Show it again and save it."
    const val SHOW_KIT = "Show the kit"
    const val KIT_UNSEEN =
        "A recovery kit was made here and not said saved before Momotus let go of it (when the app closed, or " +
            "five minutes after you left its page), so it cannot be shown again. Make a new one: Momotus removes " +
            "the unseen one."
    const val MAKE_NEW_KIT = "Make a new kit"
    const val KIT_EXPIRED =
        "A recovery kit was made here and left unsaved for five minutes after you left its page, so Momotus let " +
            "go of it and cannot show it again. Make a new one: Momotus removes the unseen one."
    const val KIT_ASK_SHOW = "The kit is shown again only after your screen lock."

    // Remove and report lost
    const val REMOVE_KIT_ASK = "Remove this recovery kit? It stops working."
    const val REMOVE_STALE =
        "The list could not be read again, so the kits named here may be out of date. Try again before you remove."
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
    const val AUTH_APPROVE = "Approve a new device"
    const val AUTH_DIFFER = "End the approval of a device"
    const val AUTH_SERVER_LIST = "Keep syncing with the sync server's device list"
    const val AUTH_LEAVE = "Stop syncing on this device"
    const val AUTH_DELETE = "Delete your sync account"
    const val AUTH_REMOVE_HERE = "Remove your library from this device"
    const val DISMISS = "Dismiss"

    /** The last read of the device list failed: what shows may be from before. */
    const val LIST_STALE =
        "This list and the 12 characters may be out of date: this device could not read the list again."

    /** The keystore kept a new record in memory only: nothing is sent until it is stored. */
    const val STORAGE =
        "This device could not store the sync account's key, so sync is paused. Try syncing again."

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

    /** The confirm line for removing another device, with the recovery kits it made, which go with it. */
    fun removeDeviceAsk(
        name: String,
        kits: Int = 0,
    ) = "Remove $name? It stops syncing and cannot read anything you change from now on. Devices it added stay." +
        when (kits) {
            0 -> ""
            1 -> " The recovery kit made on it stops working too."
            else -> " The $kits recovery kits made on it stop working too."
        }

    /** The button that continues with the server's list, naming what the user ticked. */
    fun forkButton(
        devices: Int,
        kits: Int = 0,
    ): String {
        if (devices == 0) return FORK_GO
        val also = if (kits == 0) "" else " and ${count(kits, "recovery kit", "recovery kits")}"
        return "Continue and remove ${count(devices, "device", "devices")}$also"
    }

    /** On a ticked row of the server's list: the kits that go with it. */
    fun goesWith(kits: Int) = if (kits == 1) "Its recovery kit goes with it." else "Its $kits recovery kits go with it."

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
        locale: Locale = Locale.getDefault(),
    ): String {
        // Day and time: kits made the same day read apart.
        val day =
            DateFormat
                .getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT, locale)
                .apply { timeZone = zone }
                .format(Date(addedMs))
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
    enum class Action { Round, Start, Recover, Approve, Devices, Kit, Remove, Lost, Leave, Delete, Compare, ServerList }

    /**
     * A sync failure in plain words; [code] names the ones with a fix. The core's own sentences are
     * kept; anything that may name the server's address is replaced. The statuses are the reply's [kind],
     * never read out of [message].
     */
    fun problem(
        code: String?,
        message: String,
        action: Action = Action.Round,
        kind: String? = null,
    ): String {
        when (code) {
            "dropped" -> {
                return "This device was removed from your sync account. Your library stays here as local data."
            }

            "accountGone" -> {
                return ACCOUNT_GONE
            }

            "fork" -> {
                return "The sync server lists different devices than this device knows. Compare the lists to keep " +
                    "syncing."
            }

            "wrongRelay" -> {
                return "This is not the sync server this device joined. Try again, or forget the account here and " +
                    "pair again."
            }

            "relayGone" -> {
                return AccountCopy.RELAY_GONE
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

            "kitUnknown" -> {
                return WRONG_KIT
            }

            // The app's own codes.
            "storage" -> {
                return STORAGE
            }

            "unconfirmed" -> {
                return "Nothing changed: this device did not confirm it is you."
            }

            // The core refuses a local leave with a record that is not this device's own: nothing was wiped.
            "wrongRecord" -> {
                return "This device's sync key does not match the account stored here, so nothing changed."
            }

            "rosterChanged" -> {
                return FORK_CHANGED
            }

            "noLock" -> {
                return "Set a screen lock first: Momotus asks for it before changing your devices or making a kit."
            }

            // A kit, a passkey or a wipe is the main profile's (D13, D17).
            "notOwner" -> {
                return "Only the main profile can do that."
            }
        }
        if (action == Action.Start && kind == "forbidden") {
            return "This sync server needs an invite code, or the one entered is wrong."
        }
        if (action == Action.Recover && kind == "notFound") return WRONG_KIT
        // The sync server no longer knows this device's token (a restore without it): retrying cannot help.
        if (action == Action.Compare && kind == "unauthorized") return FORK_TOKEN_LOST
        if (action == Action.Approve && kind == "notFound") {
            return "This code expired or was used already. Ask the new device for a new one."
        }
        if (message.startsWith("roster: ")) return pairingProblem("pairing: " + message.removePrefix("roster: "))
        for (kept in listOf("pairing: ", "the relay must use https")) {
            if (message.startsWith(kept)) return pairingProblem(message)
        }
        if (kind == "invalidUrl" || kind == "blockedAddress") return pairingProblem(message, kind)
        return when (action) {
            Action.Round -> {
                "The sync server did not answer. Your changes wait here and go out on the next sync."
            }

            Action.Devices -> {
                "The sync server did not answer, so this list may be out of date. Try again in a moment."
            }

            Action.Compare -> {
                "The sync server did not answer, so the lists could not be compared. Try again in a moment."
            }

            // The relay may have deleted it before its answer was lost: claim nothing.
            Action.Delete -> {
                "The deletion was not confirmed. The sync server did not answer. Try again in a moment."
            }

            else -> {
                "The sync server did not answer. Try again in a moment."
            }
        }
    }

    /**
     * Why pairing stopped, in words. The core's own pairing reasons are fixed sentences and are kept;
     * a connection or address error is replaced, since its text can name the server's address.
     */
    fun pairingProblem(
        reason: String,
        kind: String? = null,
    ): String =
        when {
            reason.startsWith("pairing: ") -> {
                reason.removePrefix("pairing: ").replaceFirstChar { it.uppercase() } + "."
            }

            reason.startsWith("the relay must use https") -> {
                problem("insecure", reason)
            }

            kind == "invalidUrl" -> {
                "That is not a sync server address. It looks like https://sync.example.org."
            }

            kind == "blockedAddress" -> {
                "This device does not connect to that address."
            }

            else -> {
                "The sync server did not answer. Check the address and try again."
            }
        }

    /** What a round's facts mean for the user (plans/core-protocol.md, `syncNow`), one line each. */
    fun notes(report: SyncReport): List<String> =
        buildList {
            if (report.refused > 0) {
                val n = report.refused
                add(
                    "${if (n == 1) "1 edit" else "$n edits"} from this device could not be sent: the sync " +
                        "server does not take ${if (n == 1) "it" else "them"}, so " +
                        "${if (n == 1) "it stays" else "they stay"} on this device only.",
                )
            }
            if (report.unreadable > 0) {
                val n = report.unreadable
                add(
                    "${if (n == 1) "1 change from another device" else "$n changes from other devices"} could not be " +
                        "read and ${if (n == 1) "was" else "were"} skipped.",
                )
            }
            if (report.newer > 0) {
                val n = report.newer
                val changes = if (n == 1) "1 change comes" else "$n changes come"
                val them = if (n == 1) "it" else "them"
                add("$changes from a newer version of Momotus. Update Momotus here to see $them.")
            }
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

                // A reason from a newer core.
                Blocked.Unknown -> {
                    add("The sync server did not take edits from this device this time, so they were not sent.")
                }

                null -> {
                    Unit
                }
            }
            if (report.again) add("The last sync stopped before it finished. It goes on with the next one.")
        }

    /** How a round ended: everything went, some changes still wait, or some failed (the web's `outcomeOf`). */
    enum class Outcome { Complete, Pending, Failed }

    fun outcome(r: SyncReport): Outcome =
        when {
            r.refused > 0 || r.unreadable > 0 -> {
                Outcome.Failed
            }

            r.waiting > 0 || r.parked > 0 || r.newer > 0 || r.blocked != null || r.rotationWaitMs > 0 || r.again -> {
                Outcome.Pending
            }

            else -> {
                Outcome.Complete
            }
        }

    /** The state line after a round: its counts when everything went, else what is wrong first. */
    fun stateLine(
        at: String,
        report: SyncReport,
    ) = when (outcome(report)) {
        Outcome.Failed -> "Synced at $at. Some changes did not sync."
        Outcome.Pending -> "Synced at $at. Some changes still wait."
        Outcome.Complete -> syncedLine(at, report)
    }

    /** Failures since this device joined, kept until the user dismisses them. */
    fun failedSince(
        refused: Int,
        unreadable: Int,
        newer: Int,
    ): String? {
        val parts =
            listOfNotNull(
                (
                    "${count(refused, "edit", "edits")} from this device could not be sent and " +
                        "stay${if (refused == 1) "s" else ""} here only"
                ).takeIf { refused > 0 },
                "${count(unreadable, "change", "changes")} from other devices could not be read"
                    .takeIf { unreadable > 0 },
                (
                    "${count(newer, "change", "changes")} from a newer version of Momotus " +
                        "wait${if (newer == 1) "s" else ""} for an update here"
                ).takeIf { newer > 0 },
            )
        return if (parts.isEmpty()) null else "Since this device joined, ${parts.joinToString(", and ")}."
    }

    /** The Settings row under Account: what is wrong or waits first, then the time. */
    fun summary(
        joined: Boolean,
        problem: String?,
        failed: String?,
        report: SyncReport?,
        time: String?,
    ): String {
        if (problem != null) return problem
        if (!joined) return OFF_LINE
        if (failed != null) return failed
        val note = report?.takeIf { outcome(it) != Outcome.Complete }?.let { notes(it).firstOrNull() }
        if (note != null) return note
        return if (time != null) "On. Synced at $time." else "On."
    }
}
