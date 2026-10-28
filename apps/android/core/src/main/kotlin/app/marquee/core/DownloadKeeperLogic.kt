package app.marquee.core

import app.marquee.core.model.DownloadState
import app.marquee.core.model.DownloadsSnapshot
import kotlin.math.roundToInt

/** What the keeper hears (plans/offline-downloads.md 5, Android details). */
sealed interface KeeperEvent {
    /** The `downloads` view's snapshot, as [KeeperFacts.of] reads it. */
    data class Facts(
        val facts: KeeperFacts,
    ) : KeeperEvent

    /** The app came to the front or left it. */
    data class Visible(
        val visible: Boolean,
    ) : KeeperEvent

    /** The system started the job again after it stopped it (the network came back). */
    data object Started : KeeperEvent

    /** The system stopped the job or the service (a quota, the network constraint, the user in Task Manager). */
    data object Stopped : KeeperEvent

    /** The system refused the job or the service: it is asked again at the next [Minute], not at every snapshot. */
    data object Refused : KeeperEvent

    /** A while after the queue emptied: a queue that only blinked empty between two downloads keeps its job. */
    data object Settled : KeeperEvent

    /** Once a minute while downloads run; a refused job is asked for again. */
    data object Minute : KeeperEvent

    /** The session's profile; a change forgets the last profile's names and notices (code review r3 M1). */
    data class Profile(
        val id: String,
    ) : KeeperEvent
}

/** What the keeper does; the job, the service and the notifications only perform these. */
sealed interface KeeperEffect {
    /**
     * A user-initiated data transfer job (API 34+) or the `dataSync` service, with the bytes it expects; [unmetered]
     * when every download waits for Wi-Fi only, so the system starts the job when Wi-Fi comes.
     */
    data class Keep(
        val bytes: Long,
        val unmetered: Boolean = false,
    ) : KeeperEffect

    /** The progress notification. */
    data class Progress(
        val title: String,
        val text: String,
        /** 0 to 1, null when no size is known yet. */
        val fraction: Float?,
    ) : KeeperEffect

    /** End the job or the service, and its notification. */
    data object Release : KeeperEffect

    /** Take down a progress notification no job or service holds (a refused or stopped hold, code review r3 m2). */
    data object HideProgress : KeeperEffect

    /** Take down the finished and failure notices: they named the last profile's titles (code review r3 M1). */
    data object ClearNotices : KeeperEffect

    /** Ask for [Settled] a few seconds from now. */
    data object SettleLater : KeeperEffect

    data class Finished(
        val title: String,
        val text: String,
    ) : KeeperEffect

    data class Failed(
        val id: String,
        val title: String,
    ) : KeeperEffect

    /** `downloadsAllowed`: whether downloads may run now. */
    data class Allowed(
        val allowed: Boolean,
    ) : KeeperEffect

    /** `StorageManager.allocateBytes`: the system frees cache for a download that waits for space. */
    data class Allocate(
        val bytes: Long,
    ) : KeeperEffect
}

/** What the keeper reads from a `downloads` snapshot. */
data class KeeperFacts(
    /** Running, queued and waiting (Wi-Fi, a connection, room, the app): all but paused (code review r1 C1). */
    val left: Int,
    /** Of [left], the ones moving now: none means every download waits (code review r3 m2). */
    val running: Int,
    val bytesDone: Long,
    val bytesTotal: Long,
    val title: String?,
    /** Done downloads: id to the title's name. */
    val done: Map<String, String>,
    /** Failed downloads: id to "Severance S1 E4". */
    val failed: Map<String, String>,
    val neededBytes: Long?,
    /** Download on Wi-Fi only is on and no download was let onto mobile data. */
    val unmetered: Boolean = false,
    /** A locked session's counts-only view: no names and nothing done or failed, so [done] and [failed] say nothing. */
    val locked: Boolean = false,
) {
    companion object {
        fun of(snapshot: DownloadsSnapshot): KeeperFacts {
            val kept = snapshot.titles.flatMap { it.rows }
            return KeeperFacts(
                left = snapshot.active.running + snapshot.active.queued + snapshot.active.waiting,
                running = snapshot.active.running,
                bytesDone = snapshot.active.bytesDone,
                bytesTotal = snapshot.active.bytesTotal,
                title = snapshot.active.title,
                done = kept.filter { it.state == DownloadState.Done }.associate { it.id to it.name },
                failed =
                    kept.filter { it.state == DownloadState.Failed }.associate { row ->
                        row.id to
                            listOfNotNull(
                                row.name,
                                row.season?.let { s -> row.episode?.let { Format.episodeLabel(s, it) } },
                            ).joinToString(" ")
                    },
                neededBytes = snapshot.storage.neededBytes,
                unmetered = snapshot.settings.wifiOnly && !snapshot.active.mobileData,
                locked = snapshot.locked,
            )
        }
    }
}

/**
 * The keeper as a pure reducer: the job or the service is held while downloads run, `downloadsAllowed` follows
 * whether the system keeps the app running (in front, or held), and the notifications say how far the batch is.
 */
class DownloadKeeperLogic {
    private var visible = false

    /** A job scheduled or running, or the service started. */
    private var held = false

    /** The network the last [KeeperEffect.Keep] asked for. */
    private var sent: Boolean? = null

    /** The system refused the last [KeeperEffect.Keep]: the next [KeeperEvent.Minute] asks again. */
    private var refused = false
    private var allowed: Boolean? = null
    private var facts: KeeperFacts? = null

    /** The batch: whether one runs, and what was done when it began (null: it began locked, names unknown). */
    private var inBatch = false
    private var doneBefore: Set<String>? = emptySet()
    private var failedSeen: Set<String>? = null

    /** The last unlocked facts: a locked snapshot's names come from here (code review r2 N1). */
    private var named: KeeperFacts? = null
    private var asked: Long? = null
    private var profile: String? = null

    /** The profile changed and no snapshot of the new one has been read yet: the last facts are the old one's. */
    private var switched = false

    fun on(event: KeeperEvent): List<KeeperEffect> =
        buildList {
            when (event) {
                is KeeperEvent.Facts -> {
                    facts(event.facts)
                }

                is KeeperEvent.Visible -> {
                    visible(event.visible)
                }

                KeeperEvent.Stopped -> {
                    held = false
                    add(KeeperEffect.HideProgress)
                }

                KeeperEvent.Refused -> {
                    held = false
                    refused = true
                    add(KeeperEffect.HideProgress)
                }

                is KeeperEvent.Profile -> {
                    profile(event.id)
                }

                KeeperEvent.Started -> {
                    held = true
                    // Started again with nothing left: it settles and lets go (code review r1 M1).
                    if ((facts?.left ?: 0) == 0) add(KeeperEffect.SettleLater)
                }

                KeeperEvent.Settled -> {
                    settled()
                }

                KeeperEvent.Minute -> {
                    refused = false
                    keep()
                    if ((facts?.left ?: 0) > 0) add(KeeperEffect.Allowed(visible || held))
                }
            }
            allowed()
        }

    /**
     * Another profile: its failures and finishes are its own, so nothing of the last profile is news to it and its
     * notices go; names and the batch's start are read again from its first snapshot (code review r3 M1).
     */
    private fun MutableList<KeeperEffect>.profile(id: String) {
        val last = profile
        profile = id
        if (last == null || last == id) return
        failedSeen = null
        named = null
        doneBefore = null
        switched = true
        add(KeeperEffect.ClearNotices)
    }

    private fun MutableList<KeeperEffect>.facts(read: KeeperFacts) {
        val before = facts.takeUnless { switched }
        // Locked, the core lists nothing: what was done and failed stays as the last unlocked view said, so an unlock
        // raises no old failure and no batch counts everything ever done (code review r2 N1).
        val now = if (read.locked) read.copy(done = named?.done.orEmpty(), failed = named?.failed.orEmpty()) else read
        facts = now
        if (!read.locked) {
            switched = false
            named = read
            // Failures from before this process are not news.
            failedSeen?.let { seen ->
                now.failed.filterKeys { it !in seen }.forEach { (id, name) -> add(KeeperEffect.Failed(id, name)) }
            }
            failedSeen = now.failed.keys
            // A batch begun locked before any name was known counts from the first names it sees.
            if (doneBefore == null) doneBefore = now.done.keys
        }
        val needed = now.neededBytes
        if (needed != null && needed != asked) add(KeeperEffect.Allocate(needed))
        asked = needed
        if (now.left == 0) {
            if (held) add(KeeperEffect.SettleLater)
            return
        }
        if (!inBatch) {
            inBatch = true
            doneBefore = if (read.locked && named == null) null else before?.done?.keys ?: now.done.keys
        }
        keep()
        if (held) add(progress(now))
    }

    /**
     * Holds the job while downloads are left and the app is in front (the system takes a job only then), and asks
     * again when the network it needs changed: a download let onto mobile data needs any network, not Wi-Fi (code
     * review r2 N2). The same job id replaces the job.
     */
    private fun MutableList<KeeperEffect>.keep() {
        val now = facts ?: return
        if (!visible || refused || now.left == 0 || (held && sent == now.unmetered)) return
        held = true
        sent = now.unmetered
        add(KeeperEffect.Keep(maxOf(0L, now.bytesTotal - now.bytesDone), now.unmetered))
    }

    private fun MutableList<KeeperEffect>.visible(now: Boolean) {
        visible = now
        // A refusal comes as the app leaves; back in front the system takes the job again (code review r3 m1).
        if (now) refused = false
        if (now && !held && (facts?.left ?: 0) > 0) {
            keep()
            if (held) facts?.let { add(progress(it)) }
        }
        // The core hears it at every foreground.
        if (now) allowed = null
    }

    private fun MutableList<KeeperEffect>.settled() {
        val now = facts ?: return
        if (now.left > 0 || !held) return
        held = false
        add(KeeperEffect.Release)
        val before = doneBefore.orEmpty()
        val finished = now.done.filterKeys { it !in before }
        if (finished.isNotEmpty()) add(finishedNotice(finished.values.toList()))
        inBatch = false
    }

    private fun MutableList<KeeperEffect>.allowed() {
        val want = visible || held
        if (want != allowed) {
            allowed = want
            add(KeeperEffect.Allowed(want))
        }
    }

    private fun progress(now: KeeperFacts): KeeperEffect.Progress {
        val fraction = if (now.bytesTotal > 0) (now.bytesDone.toFloat() / now.bytesTotal).coerceIn(0f, 1f) else null
        // Downloads done in this batch, then the one running: a cancelled one leaves the count.
        val before = doneBefore.orEmpty()
        val finished = now.done.keys.count { it !in before }
        val total = finished + now.left
        val at = finished + 1
        // Nothing moves (all wait for Wi-Fi, a connection, room): never "Downloading" (code review r3 m2).
        val title =
            when {
                now.running == 0 -> "Waiting"
                total > 1 -> "Downloading $at of $total"
                else -> "Downloading"
            }
        val text = listOfNotNull(now.title, fraction?.let { "${(it * 100).roundToInt()}%" }).joinToString(" · ")
        return KeeperEffect.Progress(title, text, fraction)
    }

    private fun finishedNotice(names: List<String>): KeeperEffect.Finished {
        val text =
            when {
                names.size == 1 -> names.single()
                names.distinct().size == 1 -> "${names.size} episodes of ${names.first()}"
                else -> "${names.size} downloads"
            }
        return KeeperEffect.Finished("Downloads finished", text)
    }
}
