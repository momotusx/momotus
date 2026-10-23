package app.marquee.macrobenchmark

import androidx.benchmark.macro.junit4.BaselineProfileRule
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The baseline profile: a start to Home and its scroll, a start onto a title page, then the TV tree's Home and
 * Discover moved through with the D-pad. The plugin writes the rules to app/src/main/generated/baselineProfiles
 * (`scripts/gw.sh :app:generateBaselineProfile`, which runs this class alone, see macrobenchmark/build.gradle.kts).
 */
@RunWith(AndroidJUnit4::class)
class BaselineProfileGenerator {
    @get:Rule
    val rule = BaselineProfileRule()

    @Test
    fun generate() =
        rule.collect(packageName = PACKAGE, includeInStartupProfile = true) {
            pressHome()
            startActivityAndWait()
            onboardIfAsked()
            openHome()
            scrollHome()
            killProcess()
            openTitle()
            // The TV tree: Home row by row and Discover's grid with the D-pad.
            killProcess()
            startTv()
            openHome()
            scrollTvHome()
            openTvDiscover()
            scrollTvGrid()
        }
}
