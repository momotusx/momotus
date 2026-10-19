package app.marquee.macrobenchmark

import androidx.benchmark.macro.BaselineProfileMode
import androidx.benchmark.macro.CompilationMode
import androidx.benchmark.macro.StartupMode
import androidx.benchmark.macro.StartupTimingMetric
import androidx.benchmark.macro.junit4.MacrobenchmarkRule
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Cold start to the first frame (the start picker on a device with two profiles), 10 runs. With no
 * compilation, and with the baseline profile the APK carries (none before it was generated).
 */
@RunWith(AndroidJUnit4::class)
class StartupBenchmark {
    @get:Rule
    val rule = MacrobenchmarkRule()

    private fun cold(mode: CompilationMode) =
        rule.measureRepeated(
            packageName = PACKAGE,
            metrics = listOf(StartupTimingMetric()),
            iterations = 10,
            startupMode = StartupMode.COLD,
            compilationMode = mode,
        ) {
            pressHome()
            startActivityAndWait()
        }

    @Test
    fun coldNoCompilation() = cold(CompilationMode.None())

    @Test
    fun coldBaselineProfile() = cold(CompilationMode.Partial(BaselineProfileMode.UseIfAvailable))
}
