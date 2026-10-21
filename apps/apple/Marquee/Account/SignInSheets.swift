#if !os(tvOS)
import AuthenticationServices
import CryptoKit
import Foundation
#if os(iOS)
import UIKit
#else
import AppKit
#endif

/// The system sheets the sign-in uses on iPhone, iPad and Mac (plans/accounts.md
/// 4.1, 4.4): Continue with Apple as one controller with the Apple ID request
/// and a passkey assertion, the passkey sheets alone, and the browser session
/// for a relay's brokered sign-in. The core runs none of these; it gets their
/// answers (an ID token, a PRF output, a redirect's fragment).
enum SignInSheets {
    /// What the combined sheet answered: the Apple ID (its token for the
    /// flow), or a Marquee passkey (its PRF output), picked in one sheet.
    enum AppleAnswer: Sendable {
        case apple(idToken: String, authorizationCode: String?)
        case passkey(prf: Data)
    }

    enum Failure: Error, Equatable {
        /// The user closed the sheet: back on the screen, no words.
        case cancelled
        /// No Marquee passkey here (the probe found none to offer).
        case noCredential
        /// A passkey that gave no PRF output: it cannot open the account.
        case noPRF
        /// The build has no link host, so no passkey can exist (`MARQUEE_LINK_HOST`).
        case noRelyingParty
        case other(String)
    }

    // MARK: Sheets

    /// Continue with Apple: the Apple ID request with the flow's nonce (no
    /// email or name, D5) and, when the build has a link host, a passkey
    /// assertion with the PRF input in the same sheet.
    @MainActor
    static func continueWithApple(nonce: String) async throws -> AppleAnswer {
        let apple = ASAuthorizationAppleIDProvider().createRequest()
        apple.requestedScopes = []
        apple.nonce = nonce
        var requests: [ASAuthorizationRequest] = [apple]
        if let passkey = assertion() { requests.append(passkey) }
        let authorization = try await Runner().run(requests, immediate: false)
        return try answer(authorization)
    }

    /// Apple's sheet alone, for a revoke or as Delete account's
    /// re-authentication: its authorization code goes to the relay.
    @MainActor
    static func appleForRevoke(nonce: String) async throws -> (idToken: String, authorizationCode: String) {
        let apple = ASAuthorizationAppleIDProvider().createRequest()
        apple.requestedScopes = []
        apple.nonce = nonce
        let authorization = try await Runner().run([apple], immediate: false)
        guard case .apple(let token, let code?) = try answer(authorization) else { throw Failure.other("no authorization code") }
        return (token, code)
    }

    /// The Apple ID alone (Link another): its token for the flow, no passkey offered.
    @MainActor
    static func appleID(nonce: String) async throws -> String {
        let apple = ASAuthorizationAppleIDProvider().createRequest()
        apple.requestedScopes = []
        apple.nonce = nonce
        guard case .apple(let token, _) = try answer(try await Runner().run([apple], immediate: false)) else { throw Failure.other("not an Apple ID") }
        return token
    }

    /// Use a passkey (sign-in): any Marquee passkey, empty `allowCredentials`.
    /// `immediate`: only one already on this device, no QR to another phone
    /// (the 3.1 probe); none answers `noCredential`.
    @MainActor
    static func passkey(immediate: Bool, allow: [Data] = []) async throws -> Data {
        guard let request = assertion(allow: allow) else { throw Failure.noRelyingParty }
        let authorization = try await Runner().run([request], immediate: immediate)
        guard case .passkey(let prf) = try answer(authorization) else { throw Failure.noPRF }
        return prf
    }

    /// Save a passkey here (3.1): `create()` with the account as `user.id`,
    /// then, when it gave no PRF output, one `get()` with only the new
    /// credential. A passkey that gives no PRF output cannot open the account.
    @MainActor
    static func createPasskey(account: Data, userName: String) async throws -> Data {
        guard let party = PasskeyContract.relyingParty else { throw Failure.noRelyingParty }
        let provider = ASAuthorizationPlatformPublicKeyCredentialProvider(relyingPartyIdentifier: party)
        let request = provider.createCredentialRegistrationRequest(challenge: PasskeyContract.challenge(), name: userName, userID: account)
        request.userVerificationPreference = .required
        request.prf = .inputValues(.init(saltInput1: PasskeyContract.prfInput))
        let authorization = try await Runner().run([request], immediate: false)
        guard let made = authorization.credential as? ASAuthorizationPlatformPublicKeyCredentialRegistration else {
            throw Failure.other("not a passkey")
        }
        if let first = made.prf?.first { return first.withUnsafeBytes { Data($0) } }
        guard made.prf?.isSupported != false else { throw Failure.noPRF }
        return try await passkey(immediate: false, allow: [made.credentialID])
    }

    /// The passkey assertion with the PRF input; none without a link host.
    @MainActor
    private static func assertion(allow: [Data] = []) -> ASAuthorizationPlatformPublicKeyCredentialAssertionRequest? {
        guard let party = PasskeyContract.relyingParty else { return nil }
        let provider = ASAuthorizationPlatformPublicKeyCredentialProvider(relyingPartyIdentifier: party)
        let request = provider.createCredentialAssertionRequest(challenge: PasskeyContract.challenge())
        request.userVerificationPreference = .required
        request.allowedCredentials = allow.map { ASAuthorizationPlatformPublicKeyCredentialDescriptor(credentialID: $0) }
        request.prf = .inputValues(.init(saltInput1: PasskeyContract.prfInput))
        return request
    }

    private static func answer(_ authorization: ASAuthorization) throws -> AppleAnswer {
        switch authorization.credential {
        case let apple as ASAuthorizationAppleIDCredential:
            guard let token = apple.identityToken.flatMap({ String(data: $0, encoding: .utf8) }) else {
                throw Failure.other("no identity token")
            }
            return .apple(idToken: token, authorizationCode: apple.authorizationCode.flatMap { String(data: $0, encoding: .utf8) })
        case let passkey as ASAuthorizationPlatformPublicKeyCredentialAssertion:
            guard let first = passkey.prf?.first else { throw Failure.noPRF }
            return .passkey(prf: first.withUnsafeBytes { Data($0) })
        default:
            throw Failure.other("an answer the sign-in does not take")
        }
    }

    // MARK: The relay's brokered sign-in

    /// Opens a brokered sign-in's page and waits for its return to
    /// `marquee://auth`: its fragment, for the view's `returned` command.
    /// Only this session's own callback is taken, never a link opened elsewhere.
    @MainActor
    static func browser(_ url: URL) async throws -> String {
        let anchor = Anchor()
        let callback: URL = try await withCheckedThrowingContinuation { continuation in
            let session = ASWebAuthenticationSession(url: url, callback: .customScheme("marquee")) { url, error in
                if let url { continuation.resume(returning: url) } else {
                    let cancelled = (error as? ASWebAuthenticationSessionError)?.code == .canceledLogin
                    continuation.resume(throwing: cancelled ? Failure.cancelled : Failure.other(error?.localizedDescription ?? "no return"))
                }
            }
            session.presentationContextProvider = anchor
            if !session.start() { continuation.resume(throwing: Failure.other("the browser did not open")) }
        }
        guard let fragment = AccountRules.returnedFragment(callback) else { throw Failure.other("not a sign-in return") }
        return fragment
    }

    // MARK: Running a controller

    /// One controller's run as an async call, presented on the key window.
    @MainActor
    private final class Runner: NSObject, ASAuthorizationControllerDelegate, ASAuthorizationControllerPresentationContextProviding {
        private var continuation: CheckedContinuation<ASAuthorization, any Error>?
        private var controller: ASAuthorizationController?
        /// Asked for a passkey already on this device: a close means none to offer.
        private var immediate = false

        func run(_ requests: [ASAuthorizationRequest], immediate: Bool) async throws -> ASAuthorization {
            try await withCheckedThrowingContinuation { continuation in
                self.continuation = continuation
                self.immediate = immediate
                let controller = ASAuthorizationController(authorizationRequests: requests)
                controller.delegate = self
                controller.presentationContextProvider = self
                self.controller = controller
                if immediate {
                    controller.performRequests(options: .preferImmediatelyAvailableCredentials)
                } else {
                    controller.performRequests()
                }
            }
        }

        nonisolated func authorizationController(controller: ASAuthorizationController, didCompleteWithAuthorization authorization: ASAuthorization) {
            MainActor.assumeIsolated {
                continuation?.resume(returning: authorization)
                continuation = nil
                self.controller = nil
            }
        }

        nonisolated func authorizationController(controller: ASAuthorizationController, didCompleteWithError error: any Error) {
            let code = (error as? ASAuthorizationError)?.code
            let words = error.localizedDescription
            MainActor.assumeIsolated {
                // Asked with preferImmediatelyAvailableCredentials: no passkey here, or none picked.
                let failure: Failure =
                    if immediate, code == .canceled || code == .notInteractive { .noCredential }
                    else if code == .canceled { .cancelled }
                    else { .other(words) }
                continuation?.resume(throwing: failure)
                continuation = nil
                self.controller = nil
            }
        }

        nonisolated func presentationAnchor(for controller: ASAuthorizationController) -> ASPresentationAnchor {
            MainActor.assumeIsolated { Anchor.window() }
        }
    }

    /// The window a sheet or a browser session sits on.
    private final class Anchor: NSObject, ASWebAuthenticationPresentationContextProviding {
        nonisolated func presentationAnchor(for session: ASWebAuthenticationSession) -> ASPresentationAnchor {
            MainActor.assumeIsolated { Anchor.window() }
        }

        @MainActor static func window() -> ASPresentationAnchor {
            #if os(iOS)
            let scenes = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
            return scenes.flatMap(\.windows).first(where: \.isKeyWindow) ?? scenes.first.map { UIWindow(windowScene: $0) } ?? ASPresentationAnchor()
            #else
            return NSApp.keyWindow ?? NSApp.windows.first ?? ASPresentationAnchor()
            #endif
        }
    }
}
#endif
