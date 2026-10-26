package app.marquee.design

import androidx.activity.ComponentActivity
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.unit.dp
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/** The loading mark draws two copies of the bird and the name, and TalkBack reads the name once. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class BrandTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    @Test
    fun `the loading mark sweeps and speaks one name`() {
        compose.setContent { MarqueeTheme(Form.Phone) { BrandLoading(72.dp) } }
        compose.onAllNodesWithTag(BRAND_SWEEP).assertCountEquals(1)
        compose.onAllNodesWithContentDescription("Momotus").assertCountEquals(1)
    }

    @Test
    fun `with animations off the loading mark is the still lockup, spoken once`() {
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                CompositionLocalProvider(LocalReducedMotion provides true) { BrandLoading(72.dp) }
            }
        }
        compose.onAllNodesWithTag(BRAND_SWEEP).assertCountEquals(0)
        compose.onAllNodesWithContentDescription("Momotus").assertCountEquals(1)
    }
}
