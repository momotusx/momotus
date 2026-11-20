package app.marquee.core

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshots.Snapshot
import app.marquee.core.client.snapshotValues
import kotlinx.coroutines.flow.filterNotNull
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.onEach
import kotlinx.coroutines.flow.take
import kotlinx.coroutines.flow.toList
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeoutOrNull
import org.junit.Test
import java.io.File
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * [snapshotValues] never loses a change another thread applies while the flow reads, as Compose's `snapshotFlow`
 * (runtime 1.12) does on its first read: with Compose's, this test failed within ten attempts.
 */
class SnapshotValuesTest {
    private class Holder {
        var value: Int? by mutableStateOf(null)
    }

    @Test
    fun `a change applied on another thread during the first read is never lost`() {
        repeat(5_000) { i ->
            val holder = Holder()
            val go = CountDownLatch(1)
            val writer =
                Thread {
                    go.await()
                    // A different short wait each time, so the apply lands all over the flow's first read.
                    val spin = System.nanoTime() + i % 50 * 1_000L
                    while (System.nanoTime() < spin) Thread.onSpinWait()
                    Snapshot.withMutableSnapshot { holder.value = 1 }
                }.apply { start() }
            val seen =
                runBlocking {
                    go.countDown()
                    withTimeoutOrNull(2_000) { snapshotValues { holder.value }.filterNotNull().first() }
                }
            writer.join()
            assertEquals(1, seen, "the change of attempt $i was lost")
        }
    }

    @Test
    fun `a change after the first read comes, and an equal value does not come again`() {
        val holder = Holder()
        val emitted = List(3) { CountDownLatch(1) }
        val runs = AtomicInteger()
        // Each step waits for the flow's last one, so no write races the read it is meant to follow.
        Thread {
            if (!emitted[0].await(5, TimeUnit.SECONDS)) return@Thread
            Snapshot.withMutableSnapshot { holder.value = 1 }
            if (!emitted[1].await(5, TimeUnit.SECONDS)) return@Thread
            // Still above zero: the flow reads again and holds the equal value back.
            Snapshot.withMutableSnapshot { holder.value = 2 }
            val deadline = System.nanoTime() + 5_000_000_000
            while (runs.get() < 3 && System.nanoTime() < deadline) Thread.onSpinWait()
            Snapshot.withMutableSnapshot { holder.value = -1 }
        }.start()
        val seen =
            runBlocking {
                var count = 0
                withTimeoutOrNull(10_000) {
                    snapshotValues {
                        runs.incrementAndGet()
                        (holder.value ?: 0) > 0
                    }.onEach { emitted[count++].countDown() }
                        .take(3)
                        .toList()
                }
            }
        assertEquals(listOf(false, true, false), seen)
    }

    @Test
    fun `no module that reads the core's state uses Compose's snapshotFlow`() {
        // design has no core state, only its own on the main thread.
        val files =
            listOf("app", "core", "player", "ui-phone", "ui-tv")
                .flatMap { m -> File("..", "$m/src/main").walkTopDown().filter { it.extension == "kt" }.toList() }
        assertTrue(files.size > 50, "found ${files.size} sources")
        val hits = files.filter { f -> f.readLines().any { it == "import androidx.compose.runtime.snapshotFlow" } }
        assertEquals(emptyList(), hits.map { it.path })
    }
}
