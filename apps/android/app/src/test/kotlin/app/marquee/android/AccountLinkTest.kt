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
    fun `a pairing code by link reaches the sheet whole, from the link host or its page, and nowhere else`() {
        val code = "https://link.example/pair#v=3&s=x&p=y&k=z&g=w&n=Den&f=tv"
        assertEquals(AccountLink.Pair(code), accountLink(view(code), "link.example"))
        // The link host's Open in Momotus, for a phone where the link did not open the app (plans/tv-qr-login.md 5.7).
        val handed = "momotus://pair#v=3&s=x&p=y&k=z&g=w"
        assertEquals(AccountLink.Pair(handed), accountLink(view(handed), ""))
        assertNull(accountLink(view("momotus://pair/x#v=3"), "link.example"))
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

/**
 * Only the camera says a code was scanned (plans/tv-qr-login.md 4): the core trusts the app's word, so no photo
 * import, paste or link may reach the sheet as a scan. Every place that marks a code scanned is a camera callback.
 */
class ScanWordTest {
    @Test
    fun `only the camera marks a code scanned`() {
        val sources =
            java
                .io
                .File("src/main/kotlin")
                .walk()
                .filter { it.extension == "kt" }
                .flatMap { file -> file.readLines().map { file.name to it.trim() } }
                .toList()
        val scans = sources.filter { (_, line) -> "scanned = true" in line || "scannedHere(" in line }
        assertEquals(
            listOf(
                "SyncScreens.kt" to "camera { approval.offer(it, scanned = true) }",
                "SyncScreens.kt" to "if (canScan) camera { approval.scannedHere(it) }",
            ),
            scans,
        )
    }
}
