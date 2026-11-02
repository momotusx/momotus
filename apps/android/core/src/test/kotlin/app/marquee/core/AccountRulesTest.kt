package app.marquee.core

import app.marquee.core.AccountRules.Act
import app.marquee.core.AccountRules.Card
import app.marquee.core.model.Announced
import app.marquee.core.model.Announcement
import app.marquee.core.model.MemberInfo
import app.marquee.core.model.ProfileInfo
import app.marquee.core.model.ScheduledDeletion
import app.marquee.core.sync.Passkeys
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import java.io.File
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertIs
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** The account words and rules (plans/accounts.md 4.1 to 4.4, 5, D13 to D17), as the web's account.test.ts. */
class AccountRulesTest {
    private val shared = Json.parseToJsonElement(File("../../shared/account-words.json").readText()).jsonObject

    @Test
    fun `the words are the shared ones, every key and no more`() {
        val theirs = shared.filterKeys { it != "about" }
        assertEquals(theirs.keys, AccountCopy.all.keys)
        for ((key, value) in theirs) {
            val ours = AccountCopy.all.getValue(key)
            when (value) {
                is JsonPrimitive -> assertEquals(value.content, ours, key)
                is JsonArray -> assertEquals(value.map { it.jsonPrimitive.content }, ours, key)
                is JsonObject -> assertEquals(value.mapValues { it.value.jsonPrimitive.content }, ours, key)
            }
        }
    }

    @Test
    fun `every key the words file's about names exists`() {
        val about = shared.getValue("about").jsonPrimitive.content
        val named =
            Regex(
                """\b(announce[A-Z]\w*|removePasskeyAsk|providerAccount\w*|notOwner)\b""",
            ).findAll(about)
                .map {
                    it.value
                }.toSet()
        assertTrue(named.isNotEmpty())
        for (key in named) assertTrue(key in AccountCopy.all, key)
    }

    @Test
    fun `the PRF input is the vectors file's`() {
        val vectors = Json.parseToJsonElement(File("../../shared/passkey-vectors.json").readText()).jsonObject
        assertEquals(
            vectors
                .getValue("prfInput")
                .jsonObject
                .getValue("base64url")
                .jsonPrimitive.content,
            Passkeys.PRF_INPUT,
        )
    }

    @Test
    fun `Android lists Google first, then Apple and Facebook, then the relay's others`() {
        assertEquals(
            listOf("google", "apple", "facebook", "mock"),
            AccountRules.providerOrder(listOf("mock", "facebook", "apple", "google")),
        )
        assertEquals("Continue with Google", AccountRules.continueWith("google"))
        assertEquals("Continue with mock", AccountRules.continueWith("mock"))
        assertEquals("mock sign-in", AccountRules.providerAccount("mock"))
    }

    @Test
    fun `Settings offers Connect Google only when the relay has Google and the read links lack it`() {
        assertTrue(AccountRules.connectGoogle(listOf("google", "email"), listOf("email")))
        assertTrue(AccountRules.connectGoogle(listOf("google"), emptyList()))
        assertFalse(AccountRules.connectGoogle(listOf("google"), listOf("google")))
        assertFalse(AccountRules.connectGoogle(listOf("facebook", "email"), emptyList()))
        // Links not read yet: no row that might name a link the account already has.
        assertFalse(AccountRules.connectGoogle(listOf("google"), null))
        assertEquals("Connect Google", AccountRules.connectTitle("google"))
        assertEquals("Sign in with your Google account too.", AccountRules.connectText("google"))
    }

    @Test
    fun `email is never a provider button, and its words are the email ones`() {
        assertEquals(listOf("google", "mock"), AccountRules.providerOrder(listOf("email", "mock", "google")))
        assertEquals(emptyList<String>(), AccountRules.providerOrder(listOf("email")))
        assertEquals("Email", AccountRules.providerName("email"))
        assertEquals(
            "This email address has made several Momotus accounts lately. Try again later or use another way.",
            AccountRules.signInFailure("signupLimit", "email")!!.text,
        )
        assertEquals("Cancel and turn off email sign-in", AccountRules.cancelUnlink("email"))
        assertEquals("Cancel and turn off Google sign-in", AccountRules.cancelUnlink("google"))
        val d = assertIs<Card.Deletion>(card(ScheduledDeletion(5, "email")))
        assertEquals(
            "Someone signed in with your email address and asked to delete your Momotus account on D5. " +
                "Not you? Cancel, and turn off email sign-in.",
            d.text,
        )
    }

    @Test
    fun `the email stages' answers, as the answers table`() {
        assertEquals(AccountCopy.BAD_ADDRESS, AccountRules.emailFieldError("badAddress"))
        assertEquals(AccountCopy.WRONG_CODE, AccountRules.emailFieldError("wrongCode"))
        assertEquals(AccountCopy.WRONG_CODE, AccountRules.emailFieldError("badCode"))
        assertNull(AccountRules.emailFieldError("ticketExpired"))
        for (code in listOf("ticketExpired", "wrongFlow")) {
            val f = AccountRules.emailFailure(code)
            assertEquals(AccountCopy.CODE_EXPIRED, f.text, code)
            assertEquals(AccountCopy.CODE_EXPIRED_ACTIONS, f.actions.map { it.label })
            assertEquals(listOf(Act.Resend, Act.OtherAddress), f.actions.map { it.act })
        }
        val refused = AccountRules.emailFailure("signInRefused")
        assertEquals(AccountCopy.SIGN_IN_REFUSED, refused.text, "keeps its own words")
        assertEquals(listOf(Act.Resend, Act.OtherAddress), refused.actions.map { it.act })
        assertEquals(listOf(Act.Other), AccountRules.emailFailure("emailLimit").actions.map { it.act })
        assertEquals(AccountCopy.EMAIL_WAIT, AccountRules.emailFailure("emailWait").text)
        assertTrue(AccountRules.emailFailure("emailWait").actions.isEmpty())
        assertEquals(listOf(Act.Retry, Act.Other), AccountRules.emailFailure("mailUnavailable").actions.map { it.act })
        assertEquals(AccountCopy.BUSY, AccountRules.emailFailure("busy").text)
        assertEquals(listOf(Act.Retry), AccountRules.emailFailure("busy").actions.map { it.act })
        assertEquals(AccountCopy.UNREACHABLE, AccountRules.emailFailure(null).text, "no answer came")
        assertTrue(AccountRules.emailEnds("ticketExpired") && AccountRules.emailEnds("emailLimit"))
        assertFalse(AccountRules.emailEnds("busy") || AccountRules.emailEnds(null))
    }

    @Test
    fun `section 5's failures and their buttons`() {
        assertNull(AccountRules.signInFailure("cancelled"))
        val expired = AccountRules.signInFailure("ticketExpired")!!
        assertEquals("Sign in again to finish.", expired.text)
        assertEquals(listOf(Act.Retry), expired.actions.map { it.act })
        val elsewhere = AccountRules.signInFailure("linkedElsewhere", "google")!!
        assertEquals("This Google account already signs in to another Momotus account.", elsewhere.text)
        assertEquals(listOf(Act.Move, Act.Cancel), elsewhere.actions.map { it.act })
        assertEquals(listOf(Act.Retry, Act.NotNow), AccountRules.signInFailure("unreachable")!!.actions.map { it.act })
        assertEquals(listOf(Act.Kit, Act.CantUse), AccountRules.signInFailure("kitUnknown")!!.actions.map { it.act })
        assertEquals(
            listOf(Act.Retry, Act.Scan),
            AccountRules.signInFailure("passkeyCancelled")!!.actions.map { it.act },
        )
        assertEquals(AccountCopy.SIGN_IN_REFUSED, AccountRules.signInFailure("whatever")!!.text)
        assertNull(AccountRules.revokeFailure("cancelled", "Unlink anyway", "Cancel"))
        assertEquals(
            listOf(
                Act.Anyway,
                Act.Cancel,
            ),
            AccountRules.revokeFailure("signInRefused", "Unlink anyway", "Cancel")!!.actions.map {
                it.act
            },
        )
    }

    @Test
    fun `a return naming another relay is another service's`() {
        assertTrue(AccountRules.sameRelay("flow=f&code=c&relay=sync.example.org", "https://sync.example.org"))
        assertFalse(AccountRules.sameRelay("flow=f&code=c&relay=evil.example", "https://sync.example.org"))
        assertTrue(AccountRules.sameRelay("flow=f&code=c", "https://sync.example.org"))
    }

    private fun profile(
        owner: Boolean = true,
        kids: Boolean = false,
    ) = ProfileInfo("p", "Robin", "a", 0, false, owner, false, kids)

    @Test
    fun `Stop syncing starts with the library kept, as its words say`() {
        assertFalse(AccountRules.REMOVE_LIBRARY_FIRST)
    }

    @Test
    fun `the rows by profile and form, D13 to D17`() {
        val owner = AccountRules.rows(profile(), tv = false, locked = false)
        assertTrue(owner.links && owner.delete && owner.makeKit && owner.savePasskey && owner.approve)
        assertTrue(owner.leave.ask)
        val other = AccountRules.rows(profile(owner = false), tv = false, locked = false)
        assertFalse(other.links || other.delete || other.makeKit || other.savePasskey)
        assertTrue(other.approve, "approving is any unlocked profile's (D17)")
        assertFalse(other.leave.ask)
        val tv = AccountRules.rows(profile(), tv = true, locked = false)
        assertFalse(tv.approve, "a TV approves nothing (D6)")
        assertFalse(tv.savePasskey)
        assertTrue(tv.leave.always)
        assertFalse(AccountRules.rows(profile(owner = false), tv = true, locked = false).leave.show)
        assertFalse(AccountRules.rows(profile(), tv = false, locked = true).delete)
        assertFalse(
            AccountRules.rows(profile(owner = false, kids = true), tv = false, locked = false).approve,
            "a kids profile opens no scanner",
        )
        // Before the profiles arrive, or for a profile that is gone: none of the owner's rows.
        val none = AccountRules.rows(AccountRules.activeProfile(null), tv = false, locked = false)
        assertFalse(none.links || none.delete || none.makeKit || none.savePasskey || none.approve)
    }

    private fun note(
        kind: Announced,
        member: String? = "m1",
        by: String? = "phone",
        name: String? = "Desk",
        kit: Boolean = false,
        passkey: Boolean = false,
    ) = Announcement("1.$member", kind, member, kit, passkey, by, name)

    private val members =
        listOf(
            MemberInfo("phone", kit = false, name = "Robin's phone", addedMs = 1, thisDevice = false),
            MemberInfo("quiet", kit = false, name = null, addedMs = 1, thisDevice = false),
        )

    @Test
    fun `each announcement's line, by the about's rules`() {
        assertEquals("A new device joined: Desk.", AccountRules.announcementLine(note(Announced.Device), members))
        assertEquals(
            "A new device joined.",
            AccountRules.announcementLine(note(Announced.Device, name = null), members),
        )
        assertEquals(
            "A passkey was added on Robin's phone.",
            AccountRules.announcementLine(note(Announced.Passkey), members),
        )
        assertEquals(
            "A recovery kit was made on a device with no name.",
            AccountRules.announcementLine(note(Announced.Kit, by = "quiet"), members),
        )
        assertEquals(
            "A recovery kit was made on a device no longer in your account.",
            AccountRules.announcementLine(note(Announced.Kit, by = "gone"), members),
        )
        assertEquals(
            "Desk signed out of your account.",
            AccountRules.announcementLine(note(Announced.Removed, by = "m1"), members),
        )
        assertEquals(
            "A passkey was removed from your account on Robin's phone.",
            AccountRules.announcementLine(note(Announced.Removed, kit = true, passkey = true), members),
        )
        assertEquals(
            "Desk was removed from your account on Robin's phone.",
            AccountRules.announcementLine(note(Announced.Removed), members),
        )
        assertEquals(
            AccountCopy.ANNOUNCE_DROPPED,
            AccountRules.announcementLine(note(Announced.Dropped, member = null, by = null), members),
        )
    }

    private fun card(
        deletion: ScheduledDeletion? = null,
        gone: Boolean = false,
        notes: List<Announcement> = emptyList(),
        who: ProfileInfo? = profile(),
        alone: Boolean = false,
    ) = AccountRules.homeCard(AccountRules.CardState(deletion, gone, notes, members, who, alone) { "D$it" })

    @Test
    fun `Home's card by priority, the owner's OK, none on a kids profile`() {
        val d =
            assertIs<Card.Deletion>(
                card(ScheduledDeletion(5, "google"), gone = true, notes = listOf(note(Announced.Device))),
            )
        assertTrue(d.text.contains("Google") && d.text.contains("D5"))
        assertEquals("Your account will be deleted on D5.", assertIs<Card.Deletion>(card(ScheduledDeletion(5))).text)
        assertTrue(assertIs<Card.Gone>(card(gone = true)).remove)
        assertFalse(assertIs<Card.Gone>(card(gone = true, who = profile(owner = false))).remove)
        val passkey = assertIs<Card.Notice>(card(notes = listOf(note(Announced.Passkey))))
        assertTrue(passkey.ok && passkey.notYou)
        val other = assertIs<Card.Notice>(card(notes = listOf(note(Announced.Passkey)), who = profile(owner = false)))
        assertFalse(other.ok, "only the owner dismisses a security notice")
        assertFalse(other.notYou)
        assertNull(
            card(notes = listOf(note(Announced.Device)), who = profile(kids = true)),
            "a kids profile shows no card",
        )
        assertNull(card(deletion = ScheduledDeletion(5), who = profile(kids = true)))
        assertIs<Card.SecondWay>(card(alone = true))
        assertNull(card(alone = true, who = profile(owner = false)), "the nudge is the owner's")
    }

    @Test
    fun `a second way in is a recovery kit, a passkey that opened, or another phone or tablet`() {
        val me = MemberInfo("me", kit = false, addedMs = 1, thisDevice = true, form = "phone")
        assertTrue(AccountRules.needsSecondWayIn(listOf(me)))
        assertTrue(
            AccountRules.needsSecondWayIn(
                listOf(me, MemberInfo("tv", kit = false, addedMs = 1, thisDevice = false, form = "tv")),
            ),
        )
        assertFalse(
            AccountRules.needsSecondWayIn(
                listOf(me, MemberInfo("pad", kit = false, addedMs = 1, thisDevice = false, form = "tablet")),
            ),
        )
        assertFalse(
            AccountRules.needsSecondWayIn(listOf(me, MemberInfo("k", kit = true, addedMs = 1, thisDevice = false))),
        )
        val unused = MemberInfo("pk", kit = true, passkey = true, addedMs = 1, thisDevice = false)
        assertTrue(
            AccountRules.needsSecondWayIn(listOf(me, unused)),
            "a passkey that never opened the account is not yet a way in",
        )
        assertFalse(AccountRules.needsSecondWayIn(listOf(me, unused.copy(lastUsedBy = "pad"))))
        assertFalse(AccountRules.needsSecondWayIn(null))
    }
}
