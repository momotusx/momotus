package app.marquee.core

import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreJson
import app.marquee.core.model.ProfileInfo
import app.marquee.core.model.ProfilesSnapshot
import app.marquee.core.session.CoreOps
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.Executors
import kotlin.test.AfterTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** Incognito's rules on the app side (plans/batch-10-incognito.md 2.8, 5, 6): the face, the words, the ops. */
class IncognitoTest {
    private fun profile(
        id: String,
        pin: Boolean = false,
        kids: Boolean = false,
    ) = ProfileInfo(id, id.uppercase(), "letter:ember", 0, pin, id == "ana", false, kids)

    private val ana = profile("ana", pin = true)
    private val bo = profile("bo")

    private fun snapshot(
        host: String? = null,
        pin: String? = null,
        locked: Boolean = false,
        profiles: List<ProfileInfo> = listOf(ana, bo),
        ended: Boolean = false,
    ) = ProfilesSnapshot(
        profiles = profiles,
        active = if (host != null) ProfileCopy.INCOGNITO else "bo",
        locked = locked,
        max = 5,
        kids = false,
        incognitoHost = host,
        incognitoPin = pin,
        incognitoEnded = ended,
    )

    @Test
    fun `the current profile is Incognito while a host is set, with the mark and no rights`() {
        val me = AccountRules.activeProfile(snapshot(host = "bo"))
        assertEquals(ProfileCopy.incognito, me)
        assertEquals("Incognito", me?.name)
        assertFalse(me!!.isOwner || me.hasPin || me.kids)
        assertEquals(ProfileCopy.Avatar(ProfileCopy.INCOGNITO, null), ProfileCopy.parseAvatar(me.avatar))
        // Outside it, the listed active profile.
        assertEquals(bo, AccountRules.activeProfile(snapshot()))
        assertTrue(ProfileCopy.inIncognito(snapshot(host = "bo")))
        assertFalse(ProfileCopy.inIncognito(snapshot()))
        assertFalse(ProfileCopy.inIncognito(null))
    }

    @Test
    fun `a cold start ignores Incognito, only the listed profiles and the lock decide`() {
        val one = listOf(bo)
        assertFalse(ProfileCopy.startsOnPicker(snapshot(host = "bo", profiles = one)))
        assertFalse(ProfileCopy.startsOnPicker(snapshot(profiles = one, ended = true)))
        assertTrue(ProfileCopy.startsOnPicker(snapshot(host = "bo")))
    }

    @Test
    fun `Home shows no account card in Incognito`() {
        val card =
            AccountRules.CardState(
                deletion = null,
                gone = true,
                announcements = emptyList(),
                members = null,
                profile = ProfileCopy.incognito,
                alone = true,
                date = { "" },
            )
        assertNull(AccountRules.homeCard(card))
        assertTrue(AccountRules.homeCard(card.copy(profile = bo)) is AccountRules.Card.Gone)
    }

    @Test
    fun `the keypad names the PIN's profile, and says why only when the kids rule asked`() {
        val kid = profile("kid", kids = true)
        val kidsDevice = snapshot(pin = "ana", profiles = listOf(ana, bo, kid))
        assertEquals(ana, ProfileCopy.incognitoPinProfile(kidsDevice))
        assertTrue(ProfileCopy.incognitoKidsReason(kidsDevice))
        // Locked: the owner's PIN is asked because of the lock, kids or not.
        assertFalse(ProfileCopy.incognitoKidsReason(kidsDevice.copy(locked = true)))
        // No PIN asked, no reason.
        assertFalse(ProfileCopy.incognitoKidsReason(kidsDevice.copy(incognitoPin = null)))
        assertNull(ProfileCopy.incognitoPinProfile(snapshot()))
        assertEquals("Enter the PIN for ANA to open Incognito", ProfileCopy.incognitoPinTitle("ANA"))
    }

    @Test
    fun `the intro says what is kept, by form, and the debrid line only with an account`() {
        val phone = ProfileCopy.intro(debrid = false, tv = false)
        assertEquals(listOf("What you watch and search.", "Addons and settings you change."), phone.notKept)
        assertEquals(
            listOf(
                "Downloads stay on this device, and every profile can see them.",
                "Addons, your internet provider and torrent peers can still see what you play.",
            ),
            phone.seen,
        )
        assertEquals("It ends when you leave, close the app, or after 30 minutes away.", phone.ends)
        val tv = ProfileCopy.intro(debrid = true, tv = true)
        assertTrue("Your debrid account keeps its own history." in tv.seen)
        assertEquals("It ends when you leave, or after 30 minutes away.", tv.ends)
    }

    @Test
    fun `the entry codes have their words`() {
        // The owner's no-space rule: a full disk is a quiet failure.
        assertEquals("Couldn't open Incognito. Try again.", ProfileCopy.problem("noSpace", "x"))
        assertEquals("Couldn't open Incognito. Try again.", ProfileCopy.problem("incognitoFailed", "x"))
        assertEquals("Not available in Incognito.", ProfileCopy.problem("incognito", "x"))
        assertEquals(
            "Kids profiles need this PIN. Remove them first, then remove the PIN.",
            ProfileCopy.problem("kidsNeedPin", "x"),
        )
    }

    private val executor = Executors.newSingleThreadExecutor { Thread(it, "test-state") }
    private val client = CoreClient(executor.asCoroutineDispatcher())
    private val ops = CoreOps(client)
    private val sent = CopyOnWriteArrayList<JsonObject>()

    init {
        val json = CoreJson.encodeToString(ProfilesSnapshot.serializer(), snapshot(host = "bo"))
        client.attach { text ->
            val msg = CoreJson.parseToJsonElement(text).jsonObject
            sent += msg
            val id = msg["id"]?.jsonPrimitive?.longOrNull ?: return@attach
            client.onMessage("""{"id":$id,"ok":true,"result":$json}""")
        }
    }

    @AfterTest
    fun stop() {
        client.close()
        executor.shutdownNow()
    }

    @Test
    fun `the ops send what the contract names`() =
        runBlocking {
            ops.switchProfile(ProfileCopy.INCOGNITO, "1234", fromChooser = true)
            assertEquals("switchProfile", sent.last()["op"]?.jsonPrimitive?.content)
            assertEquals(JsonPrimitive(true), sent.last()["fromChooser"])
            assertEquals("incognito", sent.last()["profile"]?.jsonPrimitive?.content)
            // A plain switch says nothing of the chooser.
            ops.switchProfile("bo", null)
            assertNull(sent.last()["fromChooser"])
            assertEquals("bo", ops.leaveIncognito().incognitoHost)
            assertEquals("leaveIncognito", sent.last()["op"]?.jsonPrimitive?.content)
            ops.ackIncognitoEnded()
            assertEquals("ackIncognitoEnded", sent.last()["op"]?.jsonPrimitive?.content)
        }
}
