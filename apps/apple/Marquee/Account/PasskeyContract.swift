import CryptoKit
import Foundation

/// The passkey contract (plans/accounts.md 3.1, apps/shared/passkey-vectors.json):
/// the RP ID is the link host, the PRF input is one fixed text, the challenge
/// is random (nothing checks the assertion; only the PRF output is used).
enum PasskeyContract {
    /// The one PRF input every platform evaluates (`eval.first`).
    static let prfInputText = "marquee/passkey/v1"
    static var prfInput: Data { Data(prfInputText.utf8) }

    /// The hmac-secret salt browsers and Credential Manager make of the input,
    /// `SHA-256("WebAuthn PRF" || 0x00 || input)`. Whether Apple's native API
    /// takes the input or this salt is phase 1b's one bit; the app passes the
    /// input, as `ASAuthorizationPublicKeyCredentialPRFAssertionInput` asks.
    static var prfSalt: Data {
        Data(SHA256.hash(data: Data("WebAuthn PRF".utf8) + [0] + prfInput))
    }

    /// 32 random bytes: a challenge nothing checks.
    static func challenge() -> Data {
        var generator = SystemRandomNumberGenerator()
        return Data((0..<32).map { _ in UInt8.random(in: .min ... .max, using: &generator) })
    }

    /// A PRF output as `savePasskey` and `recover` take it: base64url, no padding.
    static func base64url(_ data: Data) -> String {
        data.base64EncodedString()
            .replacingOccurrences(of: "+", with: "-")
            .replacingOccurrences(of: "/", with: "_")
            .replacingOccurrences(of: "=", with: "")
    }

    /// `syncDevices`' `account` (base64url, no padding) as the passkey's `user.id` bytes.
    static func userID(_ account: String) -> Data? {
        var text = account.replacingOccurrences(of: "-", with: "+").replacingOccurrences(of: "_", with: "/")
        text += String(repeating: "=", count: (4 - text.count % 4) % 4)
        return Data(base64Encoded: text)
    }

    /// Whether a passkey's `user.id` is the account a sign-in found (`found` names it in hex).
    static func isFor(_ user: Data, account hex: String) -> Bool {
        user.map { String(format: "%02x", $0) }.joined() == hex.lowercased()
    }

    /// The RP ID: the link host the build carries (`MARQUEE_LINK_HOST`), none without it.
    static var relyingParty: String? {
        let host = Bundle.main.object(forInfoDictionaryKey: "MarqueeLinkHost") as? String
        return host?.isEmpty == false ? host : nil
    }
}
