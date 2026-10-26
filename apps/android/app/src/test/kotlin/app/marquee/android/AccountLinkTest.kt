package app.marquee.android

import android.app.Application
import android.content.Intent
import android.net.Uri
import app.marquee.core.AccountCopy
import app.marquee.core.route.Route
import app.marquee.design.Form
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertNull

/** The account links the activity routes (plans/accounts.md 4.2, 4.4). */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], application = Application::class)
class AccountLinkTest {
    private fun view(uri: String) = Intent(Intent.ACTION_VIEW, Uri.parse(uri))

    @Test
    fun `a Custom Tab's return is the sign-in's, and only marquee auth`() {
        val back =
            assertIs<AccountLink.Return>(
                accountLink(view("momotus://auth#flow=f&code=c&relay=sync.example.org"), "link.example"),
            )
        assertEquals("flow=f&code=c&relay=sync.example.org", back.uri.encodedFragment)
        assertNull(accountLink(view("momotus://elsewhere#flow=f"), "link.example"))
        assertNull(accountLink(view("https://link.example/auth#flow=f"), "link.example"))
    }

    @Test
    fun `the link host's pair page opens the scanner, no other host or path does`() {
        assertEquals(
            AccountLink.Pair,
            accountLink(view("https://link.example/pair#v=3&s=x&p=y&k=z&g=w"), "link.example"),
        )
        assertNull(accountLink(view("https://evil.example/pair#v=3"), "link.example"))
        assertNull(accountLink(view("https://link.example/delete"), "link.example"))
        assertNull(accountLink(view("https://link.example/pair"), ""), "a build without a link host takes none")
    }

    @Test
    fun `the shortcut opens the scanner, a launch from recents repeats nothing`() {
        assertEquals(AccountLink.Shortcut, accountLink(Intent(SIGN_IN_ANOTHER), "link.example"))
        val old = view("momotus://auth#flow=f").addFlags(Intent.FLAG_ACTIVITY_LAUNCHED_FROM_HISTORY)
        assertNull(accountLink(old, "link.example"))
    }

    @Test
    fun `a first run opens the sign-in over the doors where a sync service is known`() {
        assertEquals(listOf(Route.Onboarding), firstRun("", Form.Phone), "no service: the doors")
        assertEquals(listOf(Route.Onboarding, Route.SignIn(firstRun = true)), firstRun("sync.example.org", Form.Phone))
        assertEquals(listOf(Route.Onboarding, Route.TvSignIn), firstRun("sync.example.org", Form.Tv))
    }

    @Test
    fun `the shortcut's label is the shared words'`() {
        assertEquals(
            AccountCopy.SIGN_IN_ANOTHER,
            RuntimeEnvironment.getApplication().getString(R.string.sign_in_another),
        )
    }
}
