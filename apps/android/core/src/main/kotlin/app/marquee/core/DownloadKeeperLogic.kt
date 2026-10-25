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

    /** A while after the queue emptied: a queue that only blinked empty between two downloads keeps its job. */
    data object Settled : KeeperEvent

    /** Once a minute while downloads run. */
    data object Minute : KeeperEvent
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
) {
    companion object {
        fun of(snapshot: DownloadsSnapshot): KeeperFacts {
            val kept = snapshot.titles.flatMap { it.rows }
            return KeeperFacts(
                left = snapshot.downloading.count { it.state != DownloadState.Paused },
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
                unmetered = snapshot.settings.wifiOnly && snapshot.downloading.none { it.mobileData },
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
    private var allowed: Boolean? = null
    private var facts: KeeperFacts? = null

    /** The batch: whether one runs, and what was done when it began. */
    private var inBatch = false
    private var doneBefore: Set<String> = emptySet()
    private var failedSeen: Set<String>? = null
    private var asked: Long? = null

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
                    if ((facts?.left ?: 0) > 0) add(KeeperEffect.Allowed(visible || held))
                }
            }
            allowed()
        }

    private fun MutableList<KeeperEffect>.facts(now: KeeperFacts) {
        val before = facts
        facts = now
        // Failures from before this process are not news.
        val seen = failedSeen
        if (seen !=
            null
        ) {
            now.failed.filterKeys { it !in seen }.forEach { (id, name) -> add(KeeperEffect.Failed(id, name)) }
        }
        failedSeen = now.failed.keys
        val needed = now.neededBytes
        if (needed != null && needed != asked) add(KeeperEffect.Allocate(needed))
        asked = needed
        if (now.left == 0) {
            if (held) add(KeeperEffect.SettleLater)
            return
        }
        if (!inBatch) {
            inBatch = true
            doneBefore = before?.done?.keys ?: now.done.keys
        }
        if (!held && visible) {
            held = true
            add(KeeperEffect.Keep(maxOf(0L, now.bytesTotal - now.bytesDone), now.unmetered))
        }
        if (held) add(progress(now))
    }

    private fun MutableList<KeeperEffect>.visible(now: Boolean) {
        visible = now
        val left = facts?.left ?: 0
        if (now && left > 0 && !held) {
            held = true
            add(
                KeeperEffect.Keep(
                    facts?.let { maxOf(0L, it.bytesTotal - it.bytesDone) } ?: 0L,
                    facts?.unmetered == true,
                ),
            )
            facts?.let { add(progress(it)) }
        }
        // The core hears it at every foreground.
        if (now) allowed = null
    }

    private fun MutableList<KeeperEffect>.settled() {
        val now = facts ?: return
        if (now.left > 0 || !held) return
        held = false
        add(KeeperEffect.Release)
        val finished = now.done.filterKeys { it !in doneBefore }
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
        val finished = now.done.keys.count { it !in doneBefore }
        val total = finished + now.left
        val at = finished + 1
        val title = if (total > 1) "Downloading $at of $total" else "Downloading"
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
