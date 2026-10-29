package app.marquee.ui.phone

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.compositeOver
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInRoot
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertHasClickAction
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.getUnclippedBoundsInRoot
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.unit.dp
import app.marquee.core.route.Route
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * The phone's tab bar is the iPhone's five slots (D6): four places, then the profile, whose menu opens Settings
 * (plans/nav-profile.md). Calendar is a row in Library, which is lit there.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class PhoneShellTest {
    @get:Rule
    val compose = createComposeRule()

    @Test
    fun `the bar holds four places and the profile, and Library is lit while Calendar shows`() {
        compose.setContent {
            MarqueeTheme(Form.Phone) { PhoneShell(selected = Route.Calendar, showTabs = true, onSelect = {}) {} }
        }
        compose.onAllNodes(SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.Tab)).assertCountEquals(4)
        compose.onAllNodesWithContentDescription("Settings").assertCountEquals(0)
        compose.onAllNodesWithContentDescription("Calendar").assertCountEquals(0)
        compose.onNodeWithContentDescription("Library").assertIsSelected()
    }

    private fun linkGoesBack(strip: Boolean) {
        var backs = 0
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                PhoneShell(Route.Home, showTabs = false, onSelect = {}, statusStrip = strip, onBack = { backs++ }) {}
            }
        }
        back().assertHasClickAction().performClick()
        compose.runOnIdle { assertEquals(1, backs) }
    }

    /** Back: the word in a bar, the round button's spoken name over art. */
    private fun back() = compose.onNode(hasText("Back") or hasContentDescription("Back"))

    @Test
    fun `a page under the status bar shows the back link and it goes back`() = linkGoesBack(strip = true)

    @Test
    fun `a title page with art under the status bar shows the link and it goes back`() = linkGoesBack(strip = false)

    @Test
    fun `a page without the link draws none`() {
        compose.setContent {
            MarqueeTheme(Form.Phone) { PhoneShell(selected = Route.Home, showTabs = false, onSelect = {}) {} }
        }
        compose.onAllNodes(hasText("Back") or hasContentDescription("Back")).assertCountEquals(0)
    }

    @Test
    fun `a page keeps its state when the back link comes, goes and changes form`() {
        var link by mutableStateOf<(() -> Unit)?>(null)
        var strip by mutableStateOf(true)
        val states = mutableSetOf<Any>()
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                PhoneShell(Route.Home, showTabs = false, onSelect = {}, statusStrip = strip, onBack = link) {
                    states += remember { Any() }
                }
            }
        }
        compose.runOnIdle { link = {} }
        compose.runOnIdle { strip = false }
        compose.runOnIdle { strip = true }
        compose.runOnIdle { link = null }
        compose.waitForIdle()
        assertEquals("one page state through every change", 1, states.size)
    }

    @Test
    fun `over art Back is one round button whose white chevron keeps 4_5 to 1 over pure white art`() {
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                PhoneShell(Route.Home, showTabs = false, onSelect = {}, statusStrip = false, onBack = {}) {}
            }
        }
        val round = compose.onNodeWithTag(BACK_ROUND_TAG).getUnclippedBoundsInRoot()
        assertEquals(round.right - round.left, round.bottom - round.top)
        assertEquals(BACK_HEIGHT, round.bottom - round.top)
        val circle = Color.Black.copy(alpha = BACK_PILL_ALPHA).compositeOver(Color.White)
        val ratio = (Color.White.luminance() + 0.05f) / (circle.luminance() + 0.05f)
        assertTrue("the chevron on the circle is $ratio to 1", ratio >= 4.5f)
    }

    /**
     * Offline, the shell's banner sits at the top and the page starts under it: it covers nothing at rest
     * (plans/offline-downloads.md 3.5).
     */
    @Test
    fun `the banner takes its own height and the page starts under it`() {
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                PhoneShell(
                    selected = Route.Home,
                    showTabs = true,
                    onSelect = {},
                    banner = { Box(Modifier.fillMaxWidth().height(40.dp)) },
                ) { Box(Modifier.fillMaxWidth().height(10.dp).testTag("page")) }
            }
        }
        val banner = compose.onNodeWithTag("shell-banner").getUnclippedBoundsInRoot()
        val page = compose.onNodeWithTag("page").getUnclippedBoundsInRoot()
        assertEquals(40.dp, banner.bottom - banner.top)
        assertEquals(banner.bottom, page.top)
    }

    /** The page is placed under the banner from its first frame, never under it for one (code review r6 m8). */
    @Test
    fun `the page is under the banner from its first placement`() {
        val tops = mutableListOf<Float>()
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                PhoneShell(
                    selected = Route.Home,
                    showTabs = true,
                    onSelect = {},
                    banner = { Box(Modifier.fillMaxWidth().height(40.dp)) },
                ) {
                    Box(Modifier.fillMaxWidth().height(10.dp).onGloballyPositioned { tops += it.positionInRoot().y })
                }
            }
        }
        compose.waitForIdle()
        val bannerBottom = compose.onNodeWithTag("shell-banner").getUnclippedBoundsInRoot().bottom
        assertEquals(bannerBottom.value, tops.first() / compose.density.density, 0.5f)
    }

    /** Offline, a pushed page's Back sits under the banner, never under its cover (the iPhone's r5 M1, on Android). */
    private fun backUnderBanner(strip: Boolean) {
        var backs = 0
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                PhoneShell(
                    Route.Home,
                    showTabs = false,
                    onSelect = {},
                    statusStrip = strip,
                    onBack = { backs++ },
                    banner = { Box(Modifier.fillMaxWidth().height(40.dp)) },
                ) {}
            }
        }
        val banner = compose.onNodeWithTag("shell-banner").getUnclippedBoundsInRoot()
        val bounds = back().getUnclippedBoundsInRoot()
        assertTrue("Back at ${bounds.top}, the banner ends at ${banner.bottom}", bounds.top >= banner.bottom)
        back().performClick()
        compose.runOnIdle { assertEquals(1, backs) }
    }

    @Test
    fun `offline, the back link row sits under the banner`() = backUnderBanner(strip = true)

    @Test
    fun `offline, the floating back link over art sits under the banner`() = backUnderBanner(strip = false)

    @Test
    fun `with no banner the page starts at the top`() {
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                PhoneShell(selected = Route.Home, showTabs = true, onSelect = {}) {
                    Box(Modifier.fillMaxWidth().height(10.dp).testTag("page"))
                }
            }
        }
        assertEquals(0.dp, compose.onNodeWithTag("page").getUnclippedBoundsInRoot().top)
    }
}
