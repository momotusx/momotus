package app.marquee.core

import app.marquee.core.client.CoreClient
import app.marquee.core.model.PairingSnapshot
import app.marquee.core.session.CoreOps
import app.marquee.core.sync.AccountActions
import app.marquee.core.sync.AccountSignIn
import app.marquee.core.sync.AccountSignIn.Step
import app.marquee.core.sync.BrokeredRun
import app.marquee.core.sync.GoogleSheet
import app.marquee.core.sync.LibraryRemoval
import app.marquee.core.sync.PasskeyResult
import app.marquee.core.sync.SignInPlatform
import app.marquee.core.sync.SyncAccount
import app.marquee.core.views.PairingView
import app.marquee.core.views.ProfilesView
import app.marquee.core.views.SignInView
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.Assume.assumeTrue
import java.io.File
import java.net.ServerSocket
import java.net.URI
import java.net.http.HttpClient
import java.net.http.HttpRequest
import java.net.http.HttpResponse
import java.net.http.WebSocket
import java.nio.file.Files
import java.security.SecureRandom
import java.util.concurrent.CompletionStage
import java.util.concurrent.Executors
import kotlin.test.AfterTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * Accounts on Android for real (plans/accounts.md phase 3), without a device: a relay with sign-in, the mock
 * OpenID provider (`cargo run -p mq-sync-server --example mock-oidc`, also behind the relay's Apple entry),
 * and a core per device through the dev bridge, reached over a WebSocket by the app's own [CoreClient] and
 * driven by the app's own [AccountSignIn], [AccountActions] and [LibraryRemoval]. The provider page is
 * walked by HTTP (the mock's `choose`), and its redirect handed back as the auth session would.
 *
 * Runs when `MQ_ACCOUNTS_BIN` names a target dir holding `mq-sync-server`, `mq-bridge` and
 * `examples/mock-oidc` (`cargo build -p mq-sync-server --bins --example mock-oidc -p mq-bridge`); skipped otherwise.
 */
class AccountsRelayTest {
    private val bin = System.getenv("MQ_ACCOUNTS_BIN")?.let(::File)
    private val procs = mutableListOf<Process>()
    private val scratch = Files.createTempDirectory("mq-android-accounts").toFile()
    private val http = HttpClient.newBuilder().followRedirects(HttpClient.Redirect.NEVER).build()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private val clients = mutableListOf<AutoCloseable>()

    @AfterTest
    fun stop() {
        clients.forEach { runCatching { it.close() } }
        scope.cancel()
        procs.forEach { it.destroy() }
        scratch.deleteRecursively()
    }

    private fun port() = ServerSocket(0).use { it.localPort }

    /** Starts [cmd] and waits for a stdout line holding [word]; returns that line. */
    private fun start(
        cmd: List<String>,
        word: String,
        env: Map<String, String> = emptyMap(),
    ): String {
        // Each process's own log, kept where MQ_ACCOUNTS_LOGS says (else with the run's scratch files).
        val logs = System.getenv("MQ_ACCOUNTS_LOGS")?.let(::File) ?: scratch
        val log = File(logs.apply { mkdirs() }, "${File(cmd[0]).name}-${procs.size}.log")
        val p =
            ProcessBuilder(cmd)
                .redirectError(ProcessBuilder.Redirect.appendTo(log))
                .apply {
                    environment().putAll(env)
                }.start()
        procs += p
        val reader = p.inputStream.bufferedReader()
        val deadline = System.currentTimeMillis() + 60_000
        while (System.currentTimeMillis() < deadline) {
            val line = reader.readLine() ?: error("${cmd[0]} ended before $word")
            if (word in line) {
                // Keep draining, so a full pipe never stalls the process.
                Thread { reader.forEachLine { log.appendText(it + "\n") } }.apply { isDaemon = true }.start()
                return line
            }
        }
        error("no $word from ${cmd[0]}")
    }

    /** One device: a core of its own behind the bridge, and the app's client on it. */
    private inner class Device(
        name: String,
    ) {
        private val executor = Executors.newSingleThreadExecutor { Thread(it, "state-$name") }
        val client = CoreClient(executor.asCoroutineDispatcher())
        val ops = CoreOps(client)
        val sync = SyncAccount(ops, MemoryStore(), scope)

        init {
            val port = port()
            val data = File(scratch, name).apply { mkdirs() }
            start(
                listOf(File(bin, "mq-bridge").path),
                "bridge_ready",
                mapOf(
                    "MQ_DATA" to data.path,
                    "MQ_BRIDGE_PORT" to "$port",
                ),
            )
            val text = StringBuilder()
            val socket =
                HttpClient
                    .newHttpClient()
                    .newWebSocketBuilder()
                    .buildAsync(
                        URI("ws://127.0.0.1:$port/ws"),
                        object : WebSocket.Listener {
                            override fun onText(
                                ws: WebSocket,
                                data: CharSequence,
                                last: Boolean,
                            ): CompletionStage<*>? {
                                text.append(data)
                                if (last) {
                                    client.onMessage(text.toString())
                                    text.setLength(0)
                                }
                                ws.request(1)
                                return null
                            }
                        },
                    ).join()
            // One send at a time: the JDK's WebSocket refuses a second while one is pending.
            val sending = Any()
            client.attach { msg -> synchronized(sending) { socket.sendText(msg, true).join() } }
            clients +=
                AutoCloseable {
                    socket.abort()
                    client.close()
                    executor.shutdownNow()
                }
        }
    }

    /**
     * The provider page, walked as the person [who] would: the mock's choice, then the redirects back to the app.
     * Apple answers with a form it posts at once (`response_mode=form_post`): posted here as the browser would.
     */
    private fun walk(
        url: String,
        who: String,
    ): String {
        var request = HttpRequest.newBuilder(URI("$url&choose=$who")).GET().build()
        repeat(8) {
            val r = http.send(request, HttpResponse.BodyHandlers.ofString())
            val location = r.headers().firstValue("location").orElse(null)
            if (location != null) {
                if (location.startsWith("marquee://")) return location
                request = HttpRequest.newBuilder(request.uri().resolve(location)).GET().build()
                return@repeat
            }
            val form =
                Regex("""<form method="post" action="([^"]+)">""").find(r.body())
                    ?: error("no redirect or form from ${request.uri()} (${r.statusCode()})")
            val fields =
                Regex(
                    """<input type="hidden" name="([^"]+)" value="([^"]*)">""",
                ).findAll(r.body()).joinToString("&") { m ->
                    m.groupValues[1] + "=" + java.net.URLEncoder.encode(m.groupValues[2], "UTF-8")
                }
            request =
                HttpRequest
                    .newBuilder(URI(form.groupValues[1].replace("&amp;", "&")))
                    .header("content-type", "application/x-www-form-urlencoded")
                    .POST(HttpRequest.BodyPublishers.ofString(fields))
                    .build()
        }
        error("the walk never came back to the app")
    }

    /** The platform of a phone with no Google sheet and no link host (so no passkeys), whose browser picks [who]. */
    private inner class Phone(
        var who: String,
    ) : SignInPlatform {
        @Volatile
        var run: () -> BrokeredRun? = { null }

        override fun openBrowser(url: String) {
            scope.launch(Dispatchers.IO) {
                try {
                    val back = walk(url, who)
                    println(
                        "walked $who to ${back.substringBefore('#')} with " +
                            back.substringAfter('#').split("&").joinToString(",") { f ->
                                if (f.startsWith("error") ||
                                    f.startsWith("relay")
                                ) {
                                    f
                                } else {
                                    f.substringBefore("=")
                                }
                            },
                    )
                    // Back in the app: the auth session hands its own redirect over.
                    withTimeout(10_000) { while (run() == null) delay(10) }
                    check(run()!!.returned(back)) { "the run refused its own return" }
                } catch (e: Exception) {
                    println("the walk failed: $e")
                }
            }
        }

        override suspend fun passkeyGet(
            request: String,
            immediate: Boolean,
        ) = PasskeyResult.None

        override suspend fun passkeyCreate(request: String) = PasskeyResult.Cancelled

        override val google: GoogleSheet? = null
        override val rpId = ""
    }

    private fun waitUntil(
        what: String,
        check: () -> Boolean,
    ) {
        println("waiting: $what")
        try {
            runBlocking { withTimeout(30_000) { while (!check()) delay(20) } }
        } catch (e: kotlinx.coroutines.TimeoutCancellationException) {
            throw AssertionError("timed out: $what; ${progress()}", e)
        }
    }

    /** What the flows under test said last, for a timeout's message. */
    private var progress: () -> String = { "" }

    @Test
    fun `the brokered flow, Create, a QR join that links, found on a third device, Apple's revoke, and the removal`() {
        assumeTrue(
            "MQ_ACCOUNTS_BIN names the relay, mock and bridge builds",
            bin != null && File(bin, "mq-bridge").exists(),
        )
        // The flows' own lines (codes, never secrets) go with the run's output.
        app.marquee.core.client.CoreLog.sink = { level, message, _ -> println("log $level: $message") }
        val mockUrl =
            Regex(
                """"url":"([^"]+)"""",
            ).find(start(listOf(File(bin, "examples/mock-oidc").path, "${port()}"), "mock_oidc_ready"))!!.groupValues[1]
        val relayPort = port()
        val server = "http://127.0.0.1:$relayPort"
        // A P-256 key for the relay's Apple entry, as PKCS#8 with its public key inside (ring needs it; the
        // JDK's encoding leaves it out).
        val apple =
            ProcessBuilder(
                "sh",
                "-c",
                "openssl ecparam -name prime256v1 -genkey -noout | openssl pkcs8 -topk8 -nocrypt",
            ).start()
                .let {
                    it.inputStream
                        .bufferedReader()
                        .readText()
                        .also { _ -> it.waitFor() }
                }
        check("BEGIN PRIVATE KEY" in apple) { "openssl made no key" }
        val identity = File(scratch, "identity.json")
        val key = ByteArray(32).also(SecureRandom()::nextBytes).joinToString("") { "%02x".format(it) }
        identity.writeText(
            """{"key":"$key","publicUrl":"$server","linkHost":"link.example",
               "apple":{"teamId":"TEAM","keyId":"KEY1","key":${kotlinx.serialization.json.JsonPrimitive(
                apple,
            )},"servicesId":"apple-web","bundleIds":["app.ios"],"issuer":"$mockUrl"},
               "oidc":[{"id":"mock","issuer":"$mockUrl","clientId":"marquee","clientSecret":"ms"}]}""",
        )
        start(
            listOf(
                File(bin, "mq-sync-server").path,
                "--db",
                File(scratch, "relay.sqlite").path,
                "--listen",
                "127.0.0.1:$relayPort",
                "--identity",
                identity.path,
                "--public-host",
                "127.0.0.1",
            ),
            "sync_server_ready",
            mapOf("RUST_LOG" to (System.getenv("MQ_ACCOUNTS_RUST_LOG") ?: "warn")),
        )

        fun signIn(
            d: Device,
            phone: Phone,
        ): AccountSignIn {
            lateinit var s: AccountSignIn
            s =
                AccountSignIn(
                    d.ops,
                    d.sync,
                    scope,
                    phone,
                    server,
                    "Device",
                    "phone",
                    ownerName = { "Owner" },
                    brokered = { provider, open ->
                        BrokeredRun(
                            server,
                            open,
                        ) { onPhase ->
                            SignInView(d.client, scope, server, provider, false, onPhase).also {
                                it.open()
                            }
                        }
                    },
                    nativeGoogle = false,
                )
            phone.run = { s.run }
            return s
        }

        // Desk: alice signs in with the mock provider, the relay's providers in Android's order, and creates an account.
        val desk = Device("desk")
        val deskPhone = Phone("alice")
        val a = signIn(desk, deskPhone)
        progress = { "a: ${a.step} ${a.failure?.text}" }
        a.start()
        waitUntil("providers") { a.step is Step.Start }
        assertEquals(listOf("apple", "mock"), (a.step as Step.Start).providers)
        a.choose("mock")
        waitUntil("new: alice has no account yet") { a.step == Step.New }
        a.create()
        // No passkey on this platform: the second-way step, then Later.
        waitUntil("the second-way step") { a.step == Step.SecondWay }
        a.later()
        assertTrue(desk.sync.joined)
        val deskLinks = runBlocking { desk.sync.identities() }
        assertEquals(listOf("mock"), deskLinks.map { it.provider })
        val account = runBlocking { desk.sync.loadDevices() }.account

        // Pad: bob signs in, has no account, and joins Desk's by the QR; bob's sign-in is then linked to it.
        val pad = Device("pad")
        val b = signIn(pad, Phone("bob"))
        progress = { "b: ${b.step} ${b.failure?.text}" }
        b.start()
        waitUntil("providers") { b.step is Step.Start }
        b.choose("mock")
        waitUntil("new: bob") { b.step == Step.New }
        var shown: PairingSnapshot? = null
        val pairing = PairingView(pad.client, scope, server, "Pad", "tablet").also { it.open() }
        waitUntil("the QR") {
            shown = pairing.snapshot
            shown is PairingSnapshot.Waiting
        }
        val code = runBlocking { desk.sync.approvePairing((shown as PairingSnapshot.Waiting).qr) }
        waitUntil("Pad's digits") { pairing.snapshot is PairingSnapshot.Confirm }
        assertEquals(code, (pairing.snapshot as PairingSnapshot.Confirm).code, "both screens show the same six digits")
        pairing.confirm()
        waitUntil("joined") { pairing.snapshot is PairingSnapshot.Joined }
        runBlocking { pad.sync.join((pairing.snapshot as PairingSnapshot.Joined).record).join() }
        b.joined()
        waitUntil("bob linked") { b.step == Step.Done(created = false) }
        assertEquals(2, runBlocking { desk.sync.identities() }.size, "alice and bob both sign in to the account")
        assertEquals(account, runBlocking { pad.sync.loadDevices() }.account)

        // TV-less third phone: alice again is `found`, for Desk's account; with no passkey, the QR's screen.
        val third = Device("third")
        val c = signIn(third, Phone("alice"))
        progress = { "c: ${c.step} ${c.failure?.text}" }
        c.start()
        waitUntil("providers") { c.step is Step.Start }
        c.choose("mock")
        waitUntil("found") { c.step is Step.Found }
        // `found` names it in hex, `syncDevices` in base64url: the same account.
        assertTrue(
            app.marquee.core.sync.Passkeys
                .sameAccount(account, (c.step as Step.Found).account),
        )
        assertEquals(AccountCopy.KIT_UNKNOWN_PASSKEY, c.failure!!.text)

        // Desk links Apple (the mock behind it), then unlinks it with Apple's revoke receipt.
        val deskApple = Phone("alice")
        lateinit var actions: AccountActions
        actions =
            AccountActions(
                desk.ops,
                desk.sync,
                scope,
                deskApple,
                server = { server },
                deviceName = "Desk",
                ownerName = { "Owner" },
                brokered = { provider, revoke, open ->
                    BrokeredRun(
                        server,
                        open,
                    ) { onPhase ->
                        SignInView(desk.client, scope, server, provider, revoke, onPhase).also {
                            it.open()
                        }
                    }.also { r -> scope.launch { println("ended $provider (revoke $revoke): ${r.end.await()}") } }
                },
            )
        deskApple.run = { actions.run }
        progress = { "actions: ${actions.links} ${actions.failure?.text} ${actions.said}" }
        actions.load()
        waitUntil("links") { actions.links?.size == 2 }
        deskApple.who = "carol"
        actions.link("apple")
        waitUntil("apple linked") { actions.links?.any { it.provider == "apple" } == true }
        val appleLink = actions.links!!.first { it.provider == "apple" }
        actions.unlink(appleLink)
        waitUntil("apple unlinked") { actions.links?.none { it.provider == "apple" } == true && !actions.busy }
        assertEquals(null, actions.failure, "the revoke gave a receipt, so no Unlink anyway")

        // Pad signs out and removes its library: the three steps, then a device as new.
        val removal =
            LibraryRemoval(pad.ops, pad.sync, scope, { profiles ->
                profiles.forEach {
                    pad.ops.traktSignOut(
                        it.takeIf { p ->
                            p !=
                                "default"
                        },
                    )
                }
                ; true
            }, {})
        runBlocking { removal.signOutAndRemove(force = true, debrid = true, profiles = listOf("default")) }
        assertTrue(removal.wiped)
        assertTrue(!pad.sync.joined)
        val view = ProfilesView(pad.client, scope).also { it.open() }
        waitUntil("the profiles view") { view.snapshot != null }
        assertEquals(null, view.snapshot!!.wipePending, "the removal finished: nothing pending")
    }
}
