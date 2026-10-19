package app.marquee.core.model

import kotlinx.serialization.json.JsonObject

/**
 * An opaque record the app keeps in its keystore and hands back verbatim: the Trakt record of
 * `TraktStatus.record` and `setTrakt`. Secret: stored by `SecretStore`, never logged.
 */
typealias KeystoreRecord = JsonObject
