package app.marquee.core.client

import androidx.compose.runtime.snapshots.Snapshot
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.flow
import java.util.concurrent.atomic.AtomicReference

/**
 * Compose's `snapshotFlow`, without its lost first change: [block]'s value now and again after every applied snapshot
 * that changed what it read, equal values once.
 *
 * Compose's (runtime 1.12) publishes what a run read only after the run, so a snapshot another thread applies during
 * the first run reaches nobody: the flow keeps the old value until something else changes. The core client applies
 * on its state thread, so a player sat on its spinner with the stream's snapshot already there (SubtitleFailureTest
 * on CI); a probe lost 321 of 5000 first changes. Here every apply during a run counts, so the run is repeated.
 */
fun <T> snapshotValues(block: () -> T): Flow<T> =
    flow {
        val rerun = Channel<Unit>(Channel.CONFLATED)
        // What the last run read; null while a run is going on.
        val watched = AtomicReference<Set<Any>?>(null)
        val observer =
            Snapshot.registerApplyObserver { changed, _ ->
                val reads = watched.get()
                if (reads == null || reads.any { it in changed }) rerun.trySend(Unit)
            }
        try {
            var last = run(block, watched)
            emit(last)
            while (true) {
                rerun.receive()
                val next = run(block, watched)
                if (next != last) {
                    last = next
                    emit(next)
                }
            }
        } finally {
            observer.dispose()
        }
    }

private fun <T> run(
    block: () -> T,
    watched: AtomicReference<Set<Any>?>,
): T {
    watched.set(null)
    val reads = HashSet<Any>()
    val snapshot = Snapshot.takeSnapshot { reads += it }
    val value =
        try {
            snapshot.enter(block)
        } finally {
            snapshot.dispose()
        }
    watched.set(reads)
    return value
}
