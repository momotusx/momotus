package app.marquee.android

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import androidx.activity.ComponentActivity
import androidx.activity.result.ActivityResultLauncher
import androidx.browser.auth.AuthTabIntent
import androidx.browser.customtabs.CustomTabsClient
import androidx.browser.customtabs.CustomTabsIntent
import androidx.core.net.toUri
import androidx.credentials.CreatePublicKeyCredentialRequest
import androidx.credentials.CreatePublicKeyCredentialResponse
import androidx.credentials.CredentialManager
import androidx.credentials.CustomCredential
import androidx.credentials.GetCredentialRequest
import androidx.credentials.GetPublicKeyCredentialOption
import androidx.credentials.PublicKeyCredential
import androidx.credentials.exceptions.CreateCredentialCancellationException
import androidx.credentials.exceptions.GetCredentialCancellationException
import androidx.credentials.exceptions.NoCredentialException
import app.marquee.core.client.CoreLog
import app.marquee.core.sync.BrokeredRun
import app.marquee.core.sync.GoogleAnswer
import app.marquee.core.sync.GoogleSheet
import app.marquee.core.sync.PasskeyResult
import app.marquee.core.sync.Passkeys
import app.marquee.core.sync.SignInPlatform
import com.google.android.libraries.identity.googleid.GetGoogleIdOption
import com.google.android.libraries.identity.googleid.GoogleIdTokenCredential
import kotlinx.coroutines.CancellationException
import java.lang.ref.WeakReference

/**
 * Android's half of a sign-in (plans/accounts.md 4.4): the account passkey and Google's ID token through
 * Credential Manager, and the relay's brokered flow in an Auth Tab where Chrome has one (it reports a cancel),
 * else a Custom Tab returning to `marquee://auth`. A return reaches only the run that waits for it ([run]),
 * and [BrokeredRun.returned] takes it only from the app's own scheme and only for that relay.
 */
class AccountPlatform(
    private val app: Context,
) : SignInPlatform {
    private var activity = WeakReference<ComponentActivity>(null)
    private var authTab: ActivityResultLauncher<Intent>? = null

    /** The brokered run waiting for its return, set as each one starts (one at a time). */
    @Volatile
    var run: BrokeredRun? = null

    /** The activity registers its Auth Tab launcher here in `onCreate`, before it starts. */
    fun attach(activity: ComponentActivity) {
        this.activity = WeakReference(activity)
        authTab = AuthTabIntent.registerActivityResultLauncher(activity, ::authTabResult)
    }

    // Passkeys need Android 9 (Credential Manager's public key credentials); before it there is none to offer.
    override val rpId: String =
        BuildConfig.LINK_HOST.takeIf { Build.VERSION.SDK_INT >= Build.VERSION_CODES.P }.orEmpty()

    override fun openBrowser(url: String) {
        val a = activity.get() ?: return CoreLog.w("sign-in: no activity to open the provider page in")
        val uri = url.toUri()
        val browser = CustomTabsClient.getPackageName(a, null)
        val launcher = authTab
        if (browser != null && launcher != null && CustomTabsClient.isAuthTabSupported(a, browser)) {
            AuthTabIntent.Builder().build().launch(launcher, uri, SCHEME)
        } else {
            // No Auth Tab: the return comes as a `marquee://auth` link, taken only while a run waits.
            CustomTabsIntent.Builder().build().launchUrl(a, uri)
        }
    }

    private fun authTabResult(result: AuthTabIntent.AuthResult) {
        val waiting = run ?: return
        val back = result.resultUri
        if (result.resultCode == AuthTabIntent.RESULT_OK && back != null) {
            waiting.returned(back.toString())
        } else {
            waiting.cancel()
        }
    }

    /** A `marquee://auth` link from a Custom Tab: handed to the run that waits, if any; else nothing happens. */
    fun customTabReturn(uri: Uri): Boolean = run?.returned(uri.toString()) == true

    private val credentials by lazy { CredentialManager.create(app) }

    override suspend fun passkeyGet(
        request: String,
        immediate: Boolean,
    ): PasskeyResult {
        val a = activity.get() ?: return PasskeyResult.Failed("no activity")
        return try {
            val got =
                credentials.getCredential(
                    a,
                    GetCredentialRequest(
                        listOf(GetPublicKeyCredentialOption(request, null)),
                        null,
                        false,
                        null,
                        immediate,
                    ),
                )
            (got.credential as? PublicKeyCredential)?.let { Passkeys.used(it.authenticationResponseJson) }
                ?: PasskeyResult.None
        } catch (e: CancellationException) {
            throw e
        } catch (e: GetCredentialCancellationException) {
            PasskeyResult.Cancelled
        } catch (e: NoCredentialException) {
            PasskeyResult.None
        } catch (e: Exception) {
            CoreLog.w("passkey: ${e.javaClass.simpleName}")
            PasskeyResult.Failed(e.javaClass.simpleName)
        }
    }

    override suspend fun passkeyCreate(request: String): PasskeyResult {
        val a = activity.get() ?: return PasskeyResult.Failed("no activity")
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.P) return PasskeyResult.Failed("before Android 9")
        return try {
            val made = credentials.createCredential(a, CreatePublicKeyCredentialRequest(request))
            (made as? CreatePublicKeyCredentialResponse)?.let { Passkeys.used(it.registrationResponseJson) }
                ?: PasskeyResult.None
        } catch (e: CancellationException) {
            throw e
        } catch (e: CreateCredentialCancellationException) {
            PasskeyResult.Cancelled
        } catch (e: Exception) {
            CoreLog.w("passkey: ${e.javaClass.simpleName}")
            PasskeyResult.Failed(e.javaClass.simpleName)
        }
    }

    /** Google's ID option and the account passkey in one sheet; none in a build without Google's web client id. */
    override val google: GoogleSheet? =
        BuildConfig.GOOGLE_WEB_CLIENT_ID.takeIf { it.isNotEmpty() }?.let { client ->
            object : GoogleSheet {
                override suspend fun signIn(
                    nonce: String,
                    passkeyRequest: String?,
                ): GoogleAnswer {
                    val a = activity.get() ?: return GoogleAnswer.Failed("no activity")
                    val google =
                        GetGoogleIdOption
                            .Builder()
                            .setServerClientId(client)
                            .setNonce(nonce)
                            .setFilterByAuthorizedAccounts(false)
                            .build()
                    val options = listOfNotNull(google, passkeyRequest?.let { GetPublicKeyCredentialOption(it, null) })
                    return try {
                        when (
                            val c =
                                credentials
                                    .getCredential(
                                        a,
                                        GetCredentialRequest(options, null, false, null, false),
                                    ).credential
                        ) {
                            is PublicKeyCredential -> {
                                GoogleAnswer.Passkey(Passkeys.used(c.authenticationResponseJson))
                            }

                            is CustomCredential -> {
                                if (c.type == GoogleIdTokenCredential.TYPE_GOOGLE_ID_TOKEN_CREDENTIAL) {
                                    GoogleAnswer.IdToken(GoogleIdTokenCredential.createFrom(c.data).idToken)
                                } else {
                                    GoogleAnswer.Failed(c.type)
                                }
                            }

                            else -> {
                                GoogleAnswer.Failed(c.type)
                            }
                        }
                    } catch (e: CancellationException) {
                        throw e
                    } catch (e: GetCredentialCancellationException) {
                        GoogleAnswer.Cancelled
                    } catch (e: NoCredentialException) {
                        GoogleAnswer.NoCredential
                    } catch (e: Exception) {
                        CoreLog.w("google sign-in: ${e.javaClass.simpleName}")
                        GoogleAnswer.Failed(e.javaClass.simpleName)
                    }
                }
            }
        }

    companion object {
        /** The app's own scheme, the Auth Tab's redirect scheme: `marquee://auth`. */
        const val SCHEME = "marquee"
    }
}
