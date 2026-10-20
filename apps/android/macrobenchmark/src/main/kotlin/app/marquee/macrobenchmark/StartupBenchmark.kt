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
 * Cold start to Home with its first rows, and to a title page with its details, 10 runs each: time to the
 * first frame (TTID) and to the app's fully drawn report there (TTFD). With no compilation (as before the
 * profile), and with the baseline profile the APK carries required.
 */
@RunWith(AndroidJUnit4::class)
class StartupBenchmark {
    @get:Rule
    val rule = MacrobenchmarkRule()

    private fun cold(
        mode: CompilationMode,
        title: Boolean,
    ) = rule.measureRepeated(
        packageName = PACKAGE,
        metrics = listOf(StartupTimingMetric()),
        iterations = 10,
        startupMode = StartupMode.COLD,
        compilationMode = mode,
        setupBlock = {
            // The first run of a fresh install goes through onboarding once, outside the measure.
            if (!onboarded) {
                startActivityAndWait()
                onboardIfAsked()
                killProcess()
                onboarded = true
            }
            pressHome()
        },
    ) {
        if (title) {
            openTitle()
        } else {
            startActivityAndWait()
            openHome()
        }
    }

    @Test
    fun homeNoCompilation() = cold(CompilationMode.None(), title = false)

    @Test
    fun homeBaselineProfile() = cold(CompilationMode.Partial(BaselineProfileMode.Require), title = false)

    @Test
    fun titleNoCompilation() = cold(CompilationMode.None(), title = true)

    @Test
    fun titleBaselineProfile() = cold(CompilationMode.Partial(BaselineProfileMode.Require), title = true)

    private companion object {
        var onboarded = false
    }
}
