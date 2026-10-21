package app.marquee.core

import app.marquee.core.sync.Passkeys
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** The account passkey's WebAuthn JSON for Credential Manager (plans/accounts.md 3.1). */
class PasskeysTest {
    private fun obj(text: String) = Json.parseToJsonElement(text).jsonObject

    @Test
    fun `a new passkey is the account's, resident, user-verified, with the PRF input`() {
        val r = obj(Passkeys.create("link.example", "QUNDT1VOVA", "Robin", challenge = "Y2g"))
        assertEquals(
            "link.example",
            r
                .getValue("rp")
                .jsonObject
                .getValue("id")
                .jsonPrimitive.content,
        )
        val user = r.getValue("user").jsonObject
        assertEquals("QUNDT1VOVA", user.getValue("id").jsonPrimitive.content)
        assertEquals("Robin (Marquee)", user.getValue("name").jsonPrimitive.content)
        val sel = r.getValue("authenticatorSelection").jsonObject
        assertEquals("required", sel.getValue("residentKey").jsonPrimitive.content)
        assertEquals("required", sel.getValue("userVerification").jsonPrimitive.content)
        assertEquals("none", r.getValue("attestation").jsonPrimitive.content)
        assertFalse("excludeCredentials" in r)
        val first =
            r
                .getValue(
                    "extensions",
                ).jsonObject
                .getValue("prf")
                .jsonObject
                .getValue("eval")
                .jsonObject
                .getValue("first")
        assertEquals(Passkeys.PRF_INPUT, first.jsonPrimitive.content)
    }

    @Test
    fun `a sign-in asks any passkey of the link host, a follow-up only the new one`() {
        val any = obj(Passkeys.get("link.example"))
        assertEquals(0, any.getValue("allowCredentials").jsonArray.size)
        assertEquals("required", any.getValue("userVerification").jsonPrimitive.content)
        val one = obj(Passkeys.get("link.example", listOf("Y3JlZA")))
        assertEquals(
            "Y3JlZA",
            one
                .getValue("allowCredentials")
                .jsonArray[0]
                .jsonObject
                .getValue("id")
                .jsonPrimitive.content,
        )
        assertEquals(43, Passkeys.challenge().length, "32 bytes as base64url")
    }

    @Test
    fun `an answer gives the PRF output and the passkey's user id`() {
        val used =
            Passkeys.used(
                """{"id":"Y3JlZA","rawId":"Y3JlZA","type":"public-key","response":{"userHandle":"QUNDT1VOVA=="},
                   "clientExtensionResults":{"prf":{"results":{"first":"cHJmLW91dHB1dA=="}}}}""",
            )
        assertEquals("Y3JlZA", used.credentialId)
        assertEquals("cHJmLW91dHB1dA", used.prf, "padding dropped")
        assertTrue(Passkeys.sameAccount(used.userHandle, "QUNDT1VOVA"))
        // `found` names the account in hex: the same bytes ("ACCOUNT").
        assertTrue(Passkeys.sameAccount(used.userHandle, "4143434f554e54"))
        assertFalse(Passkeys.sameAccount(used.userHandle, "T1RIRVI"))
        assertFalse(Passkeys.sameAccount(used.userHandle, "4f54484552"))
        assertFalse(Passkeys.sameAccount(null, "QUNDT1VOVA"))
        val none = Passkeys.used("""{"id":"Y3JlZA","response":{},"clientExtensionResults":{}}""")
        assertNull(none.prf)
        assertNull(none.userHandle)
    }
}
