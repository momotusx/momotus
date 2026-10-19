package app.marquee.macrobenchmark

import androidx.benchmark.macro.CompilationMode
import androidx.benchmark.macro.FrameTimingMetric
import androidx.benchmark.macro.junit4.MacrobenchmarkRule
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/** Frames and jank of a Home scroll and of a playback start; each run leaves a Perfetto trace. */
@RunWith(AndroidJUnit4::class)
class FrameBenchmark {
    @get:Rule
    val rule = MacrobenchmarkRule()

    @Test
    fun homeScroll() =
        rule.measureRepeated(
            packageName = PACKAGE,
            metrics = listOf(FrameTimingMetric()),
            iterations = 3,
            // No kill before the measured block: the setup opened Home, the block works in it.
            startupMode = null,
            compilationMode = CompilationMode.Partial(),
            setupBlock = {
                // A fresh process each run, so the run before (the player) is not what comes back.
                killProcess()
                startActivityAndWait()
                openHome()
            },
        ) { scrollHome() }

    @Test
    fun playbackStart() =
        rule.measureRepeated(
            packageName = PACKAGE,
            metrics = listOf(FrameTimingMetric()),
            iterations = 3,
            // No kill before the measured block: the setup opened Home, the block works in it.
            startupMode = null,
            compilationMode = CompilationMode.Partial(),
            setupBlock = {
                // A fresh process each run, so the run before (the player) is not what comes back.
                killProcess()
                startActivityAndWait()
                openHome()
            },
        ) { startPlayback() }
}
