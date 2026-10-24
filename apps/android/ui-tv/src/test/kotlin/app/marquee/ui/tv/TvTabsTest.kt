package app.marquee.ui.tv

import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.unit.dp
import app.marquee.core.route.Route
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import kotlin.test.assertEquals

/**
 * The bar's tabs show their place when focus rests on them (Apple TV's tab bar, the androidx.tv TabRow's debounce):
 * no Select press, and no place opened on the way across the bar.
 */
@OptIn(ExperimentalTestApi::class)
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], qualifiers = "w960dp-h540dp")
class TvTabsTest {
    @get:Rule
    val compose = createComposeRule()

    private var selected by mutableStateOf<Route>(Route.Home)
    private val opened = mutableListOf<Route>()

    private fun show() {
        compose.setContent {
            MarqueeTheme(Form.Tv) {
                TvShell(
                    selected = selected,
                    showNav = true,
                    onSelect = {
                        opened += it
                        selected = it
                    },
                    contentEntry = { null },
                    onNavFocus = {},
                    onKey = {},
                ) {
                    // Each place: one focusable item named after it.
                    Box(Modifier.size(200.dp).semantics { contentDescription = "page $selected" }.focusable())
                }
            }
        }
        compose.mainClock.autoAdvance = false
        tab("Home").performSemanticsAction(SemanticsActions.RequestFocus)
        compose.waitForIdle()
        tab("Home").assertIsFocused()
    }

    private fun tab(name: String) = compose.onNode(hasText(name))

    private fun press(key: Key) {
        compose.onNode(isFocused()).performKeyInput { pressKey(key) }
        compose.mainClock.advanceTimeByFrame()
    }

    private fun page(route: Route) = compose.onNodeWithContentDescription("page $route")

    @Test
    fun `Right from Home onto Discover shows Discover after the settle, with no Select`() {
        show()
        press(Key.DirectionRight)
        tab("Discover").assertIsFocused()
        // Not yet: a sweep across the bar opens nothing on the way.
        compose.mainClock.advanceTimeBy(TAB_SETTLE_MS / 2)
        page(Route.Home).assertExists()
        compose.mainClock.advanceTimeBy(TAB_SETTLE_MS)
        page(Route.Discover).assertExists()
        // Focus stays on the tab; Down enters the place.
        tab("Discover").assertIsFocused()
        press(Key.DirectionDown)
        page(Route.Discover).assertIsFocused()
    }

    @Test
    fun `a sweep across the bar opens only the place it stops on`() {
        show()
        press(Key.DirectionRight)
        compose.mainClock.advanceTimeBy(TAB_SETTLE_MS / 2)
        press(Key.DirectionRight)
        compose.mainClock.advanceTimeBy(TAB_SETTLE_MS * 2)
        page(Route.Library).assertExists()
        assertEquals(listOf<Route>(Route.Library), opened)
    }

    @Test
    fun `Select on the place shown enters it`() {
        show()
        compose.onNode(isFocused()).performKeyInput { pressKey(Key.DirectionCenter) }
        compose.mainClock.advanceTimeByFrame()
        page(Route.Home).assertIsFocused()
        assertEquals(emptyList(), opened)
    }
}
