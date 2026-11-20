package app.marquee.core

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshots.Snapshot
import app.marquee.core.client.snapshotValues
import kotlinx.coroutines.flow.filterNotNull
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.take
import kotlinx.coroutines.flow.toList
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeoutOrNull
import org.junit.Test
import java.io.File
import java.util.concurrent.CountDownLatch
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
    fun `equal values come once, and a change after the first read comes`() {
        val holder = Holder()
        val seen =
            runBlocking {
                val values = snapshotValues { holder.value }
                Thread {
                    Thread.sleep(50)
                    Snapshot.withMutableSnapshot { holder.value = null }
                    Snapshot.withMutableSnapshot { holder.value = 2 }
                }.start()
                withTimeoutOrNull(5_000) { values.take(2).toList() }
            }
        assertEquals(listOf(null, 2), seen)
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
