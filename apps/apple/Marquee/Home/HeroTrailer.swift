import Foundation
import Network
import SwiftUI
#if !os(tvOS)
import WebKit
#endif

/// The hero's muted trailer (plans/appletv-polish.md 4.2, the owner's pick of 2026-10-24): YouTube's own embedded
/// player, oversized to cover the hero's frame so YouTube's bars fall outside the clip, under the page's art, which
/// fades only once YouTube's start controls have gone. Muted and read back before it plays, taking no input, back
/// to the art a second before its cap. tvOS has no WebKit: no trailer there.
enum HeroTrailers {
    /// The art above the player fades this long after YouTube says it plays (Phase 0: its controls are gone by 4 s).
    static let revealDelay: Duration = .milliseconds(4500)
    /// The player past the frame on every side.
    static let overscan: CGFloat = 1.12
    static let capSeconds = 45
    /// The device's own switch, never synced (Settings > Playback).
    static let previewsKey = "mq.trailerPreviews"

    /// A YouTube video id as YouTube writes it, or none: it goes into the player's page.
    static func checkedId(_ raw: String?) -> String? {
        guard let raw, raw.count == 11, raw.allSatisfy({ $0.isASCII && ($0.isLetter || $0.isNumber || $0 == "-" || $0 == "_") }) else { return nil }
        return raw
    }

    /// The player's size to cover a frame: 16:9, as tall as the frame (or as wide) times the overscan.
    static func coverSize(_ frame: CGSize) -> CGSize {
        let height = max(frame.height, frame.width * 9 / 16) * overscan
        return CGSize(width: height * 16 / 9, height: height)
    }
}

/// What the device and the moment allow (decision 5); every one false blocks a preview.
struct TrailerSignals: Equatable {
    var enabled: Bool
    /// 6 GB of memory or more (the player costs about 200 MB, Phase 0).
    var capable: Bool
    var visible: Bool
    var reducedMotion: Bool
    /// Expensive or Low Data network.
    var metered: Bool
    /// Low Power Mode: read for the log only, it blocks nothing (owner, 2026-10-30: the preview is muted and short).
    var lowPower: Bool
    var screenReader: Bool
    /// iOS's own Auto-Play Video Previews.
    var systemAutoplay: Bool
    var hasTrailer: Bool

    /// What holds a preview back, by name; empty when it may play.
    var blocks: [String] {
        var out: [String] = []
        if !enabled { out.append("previews off") }
        if !capable { out.append("memory") }
        if !visible { out.append("not visible") }
        if reducedMotion { out.append("reduced motion") }
        if metered { out.append("metered") }
        if screenReader { out.append("screen reader") }
        if !systemAutoplay { out.append("system autoplay off") }
        if !hasTrailer { out.append("no trailer") }
        return out
    }

    var allowed: Bool { blocks.isEmpty }

    /// `allowed`, logging what held it back, so a walk reads why nothing played.
    func allowedLogged() -> Bool {
        let held = blocks
        if !held.isEmpty {
            coreLog.notice("trailer held: \(held.joined(separator: ", "), privacy: .public) (low power \(lowPower, privacy: .public))")
        }
        return held.isEmpty
    }
}

/// The network's side of the signals, one monitor for the app.
@MainActor
final class TrailerNetwork {
    static let shared = TrailerNetwork()
    private(set) var metered = true
    private let monitor = NWPathMonitor()

    private init() {
        monitor.pathUpdateHandler = { path in
            let metered = path.isExpensive || path.isConstrained || path.status != .satisfied
            Task { @MainActor in TrailerNetwork.shared.metered = metered }
        }
        monitor.start(queue: DispatchQueue(label: "trailer-network"))
    }
}

extension TrailerSignals {
    /// The device's side of decision 5, read when a trailer is due.
    @MainActor
    static func now(visible: Bool, reducedMotion: Bool, hasTrailer: Bool) -> TrailerSignals {
        let defaults = UserDefaults.standard
        let enabled = defaults.object(forKey: HeroTrailers.previewsKey) as? Bool ?? true
        #if os(iOS)
        let screenReader = UIAccessibility.isVoiceOverRunning
        let systemAutoplay = UIAccessibility.isVideoAutoplayEnabled
        #elseif os(macOS)
        let screenReader = NSWorkspace.shared.isVoiceOverEnabled
        let systemAutoplay = true
        #else
        let screenReader = false
        let systemAutoplay = false
        #endif
        return TrailerSignals(
            enabled: enabled,
            // physicalMemory reads a little under the marketed size: 6 GB phones read about 5.6.
            capable: ProcessInfo.processInfo.physicalMemory >= 5_400_000_000,
            visible: visible,
            reducedMotion: reducedMotion,
            metered: TrailerNetwork.shared.metered,
            lowPower: ProcessInfo.processInfo.isLowPowerModeEnabled,
            screenReader: screenReader,
            systemAutoplay: systemAutoplay,
            hasTrailer: hasTrailer)
    }
}

#if !os(tvOS)
/// A player of the app (decision 6 and the one-player rule): `play` stops whatever it played first. Home's hero
/// plays on `shared`; a title page owns one for its header, then its full-screen trailer (plans/trailer-inline.md 8),
/// and lets `shared` go before it plays, so one plays at a time.
@MainActor
final class TrailerPlayer: NSObject {
    static let shared = TrailerPlayer()

    /// The web view lives only while a trailer may play: made on the first `play`, released with `release()`.
    private var webView: WKWebView?
    var web: WKWebView {
        if let webView { return webView }
        let made = makeWeb()
        webView = made
        return made
    }
    private var generation = 0
    private var state: TrailerState?
    private var onEnd: ((String) -> Void)?
    private var reveal: Task<Void, Never>?
    /// The player's page has loaded: until then calls wait, in order (a Sound press while it loads must not drop the
    /// play itself), since a call that reached the page before its script would run nothing, or worse, the browser's
    /// own `stop()`, which cancels the page's load.
    private var loaded = false
    private var waiting: [String] = []

    private func run(_ js: String) {
        guard let webView else { return }
        if loaded { webView.evaluateJavaScript(js) } else { waiting.append(js) }
    }

    private func makeWeb() -> WKWebView {
        let config = WKWebViewConfiguration()
        #if os(iOS)
        config.allowsInlineMediaPlayback = true
        config.allowsPictureInPictureMediaPlayback = false
        #endif
        config.mediaTypesRequiringUserActionForPlayback = []
        config.allowsAirPlayForMediaPlayback = false
        // A weak proxy: the content controller would keep the player alive otherwise.
        config.userContentController.add(WeakHandler(self), name: "trailer")
        let web = WKWebView(frame: .zero, configuration: config)
        #if os(iOS)
        web.isOpaque = false
        web.backgroundColor = .clear
        web.scrollView.isScrollEnabled = false
        web.isUserInteractionEnabled = false
        #endif
        web.navigationDelegate = self
        // The bundle id as the page's address: YouTube reads it as the embedding app (its Referer rule).
        web.loadHTMLString(Self.page, baseURL: URL(string: "https://\(Bundle.main.bundleIdentifier ?? "app.momotus.dev")/"))
        return web
    }

    /// Plays `id` into `state`, muted, capped at `cap` seconds (0: its own length, still ended a second early).
    /// `full`: the full-screen trailer, its time read every 250 ms, its sound turned on once it plays when `loud`
    /// (never in a walk).
    func play(_ id: String, into state: TrailerState, cap: Int = HeroTrailers.capSeconds, full: Bool = false,
              loud: Bool = false, onEnd: @escaping (String) -> Void) {
        stop()
        guard let id = HeroTrailers.checkedId(id) else { return onEnd("error") }
        generation += 1
        self.state = state
        self.onEnd = onEnd
        state.loading = true
        state.buffering = true
        state.paused = false
        _ = web
        run("play('\(id)', \(cap), \(generation), \(full), \(loud && !Self.walk))")
    }

    /// Pauses a playing trailer, plays a paused one.
    func toggle() {
        if state != nil { run("toggle()") }
    }

    /// To `seconds` in the trailer.
    func seek(_ seconds: Double) {
        if state != nil, seconds.isFinite { run("seek(\(seconds))") }
    }

    /// The full-screen trailer's own sound, never the session's (`TrailerState.shared.sound`); never in a walk.
    func loud(_ on: Bool) {
        if state != nil, !Self.walk { run(on ? "sound(true)" : "sound(false)") }
    }

    /// A walk build (`-MQMute YES`): no sound reaches any player.
    static var walk: Bool {
        #if DEBUG
        DebugHooks.mute
        #else
        false
        #endif
    }

    func stop() {
        reveal?.cancel()
        reveal = nil
        if let state {
            state.shown = false
            state.playing = false
            state.loading = false
        }
        state = nil
        onEnd = nil
        run("halt()")
    }

    /// Stops and lets the web view and its content process go: Home off screen, under the player or another
    /// page, or gone. The next `play` makes a fresh one. Whoever played hears `why`: `stopped`, or `lost` when the
    /// content process died (the full screen says it cannot play; a header does not replay this session).
    func release(why: String = "stopped") {
        let done = onEnd
        stop()
        done?(why)
        guard let webView else { return }
        webView.configuration.userContentController.removeScriptMessageHandler(forName: "trailer")
        webView.removeFromSuperview()
        self.webView = nil
        loaded = false
        waiting = []
    }

    /// Sound for this session only; never in a walk (`-MQMute YES`).
    func sound(_ on: Bool) {
        if Self.walk { return }
        run(on ? "sound(true)" : "sound(false)")
    }

    fileprivate func receive(_ body: Any) {
        guard let text = body as? String, let data = text.data(using: .utf8),
              let message = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              TrailerMessages.accepts(message, generation: generation),
              let state else { return }
        let event = message["event"] as? String
        // The walk's evidence that a trailer played (plans/appletv-polish.md 9): its state, never its id.
        if event == "seek" {
            coreLog.notice("trailer seek from \(message["from"] as? Int ?? 0, privacy: .public) to \(message["to"] as? Int ?? 0, privacy: .public)")
        } else if event != "time" {
            coreLog.notice("trailer \(event ?? "?", privacy: .public) \(message["why"] as? String ?? "", privacy: .public)")
        }
        switch event {
        case "playing":
            state.playing = true
            state.buffering = false
            reveal = Task { [weak state] in
                try? await Task.sleep(for: HeroTrailers.revealDelay)
                if !Task.isCancelled { state?.shown = true }
            }
            // A preview keeps the session's sound; the full-screen trailer brings its own (`loud`).
            if message["full"] as? Bool != true, TrailerState.shared.sound { sound(true) }
        case "time":
            state.fraction = message["fraction"] as? Double ?? 0
            state.seconds = message["t"] as? Double ?? 0
            state.length = message["d"] as? Double ?? 0
            let phase = message["state"] as? String
            state.paused = phase == "paused"
            state.buffering = phase == "buffering"
        case "end":
            let done = onEnd
            reveal?.cancel()
            state.shown = false
            state.playing = false
            state.loading = false
            self.state = nil
            onEnd = nil
            done?(message["why"] as? String ?? "error")
        default:
            break
        }
    }

    /// The player's page: the official IFrame API, muted and read back before it plays (decision 6). The same script
    /// as the Android app's (HeroTrailer.kt), held to one behaviour by apps/web/e2e/trailer-page.spec.ts.
    private static let page = """
    <!doctype html><html><head><meta name="viewport" content="width=device-width, initial-scale=1">
    <style>html,body{margin:0;height:100%;background:transparent;overflow:hidden}
    #p{position:fixed;inset:0;pointer-events:none}
    #p>div{position:absolute;inset:0;pointer-events:none}
    #p iframe{width:100%;height:100%;border:0;display:block;pointer-events:none}</style></head>
    <body><div id="p"></div><script>
    let player=null, wrap=null, timers=[], ended=true, playing=false, poll=null, api=null, gen=0, full=false, want=false;
    const say=(o)=>window.webkit.messageHandlers.trailer.postMessage(JSON.stringify(Object.assign({gen,full},o)));
    function load(){ if(api) return api; api=new Promise((ok,no)=>{ window.onYouTubeIframeAPIReady=()=>ok(window.YT);
      const s=document.createElement("script"); s.src="https://www.youtube.com/iframe_api"; s.onerror=()=>{ api=null; no(); }; document.head.append(s); }); return api; }
    function finish(why){ if(ended) return; ended=true; timers.forEach(clearTimeout); clearInterval(poll); timers=[];
      // Each play in a wrapper of its own, removed whole: its frame goes even when the player never got ready.
      const p=player, w=wrap; player=null; setTimeout(()=>{ try{p&&p.getIframe&&p.getIframe().remove()}catch(e){} try{p&&p.destroy()}catch(e){} w&&w.remove(); }, why==="cap"||why==="ended"?650:0);
      say({event:"end",why}); }
    function captionsOff(){ try{ const t=player&&player.getOption("captions","track"); if(t&&Object.keys(t).length) player.setOption("captions","track",{}); player&&player.unloadModule("captions"); }catch(e){} }
    function halt(){ finish("stopped"); }
    function sound(on){ want=on; if(!playing||!player||!player.mute) return; if(on){player.unMute();player.setVolume(100);} else {player.mute();player.setVolume(0);} }
    function toggle(){ if(!player||!player.getPlayerState) return; if(player.getPlayerState()===1) player.pauseVideo(); else player.playVideo(); }
    function seek(s){ if(!player||!player.seekTo) return; const from=player.getCurrentTime(); player.seekTo(Math.max(0,s),true); say({event:"seek",from:Math.round(from),to:Math.round(s)}); }
    async function play(id, cap, g, f, loud){
      finish("stopped"); gen=g; full=f; want=loud; ended=false; playing=false;
      wrap=document.createElement("div"); const box=document.createElement("div"); wrap.append(box); document.getElementById("p").append(wrap);
      timers.push(setTimeout(()=>{ if(!playing) finish("timeout"); }, f?15000:9000));
      let YT; try{ YT=await load(); }catch(e){ if(gen===g){ say({event:"refused",why:"api unreachable"}); finish("error"); } return; }
      if(ended||gen!==g) return;
      const vars={controls:0,playsinline:1,rel:0,enablejsapi:1,iv_load_policy:3,cc_load_policy:0,disablekb:1,fs:0}; if(cap) vars.end=cap;
      player=new YT.Player(box,{host:"https://www.youtube-nocookie.com",videoId:id,width:"100%",height:"100%",playerVars:vars,
        events:{
          onReady:(e)=>{ if(ended||gen!==g) return; const fr=e.target.getIframe(); fr.setAttribute("tabindex","-1"); fr.inert=true;
            e.target.mute(); e.target.setVolume(0); const until=Date.now()+1000;
            const check=()=>{ if(ended||gen!==g) return; if(e.target.isMuted()&&e.target.getVolume()===0) e.target.playVideo();
              else if(Date.now()<until) timers.push(setTimeout(check,25)); else finish("error"); }; check(); },
          onApiChange:()=>captionsOff(),
          onStateChange:(e)=>{ if(ended||gen!==g) return;
            if(e.data===1&&!playing){ playing=true; say({event:"playing"}); captionsOff(); if(want) sound(true);
              poll=setInterval(()=>{ if(!player) return; const t=player.getCurrentTime(), len=cap||player.getDuration(), st=player.getPlayerState();
                say({event:"time",fraction:len>0?Math.min(1,t/len):0,t,d:len,state:st===1?"playing":st===2?"paused":"buffering"});
                captionsOff(); if(len>0&&t>=len-1) finish(cap?"cap":"ended"); }, 250); }
            if(e.data===0) finish(cap?"cap":"ended"); },
          onError:(e)=>{ if(gen===g){ say({event:"refused",why:"youtube "+e.data}); finish("error"); } } } }); }
    </script></body></html>
    """
}

extension TrailerPlayer: WKNavigationDelegate {
    /// The content process died (memory): whoever plays hears an error, the next play makes a fresh one.
    func webViewWebContentProcessDidTerminate(_ webView: WKWebView) {
        // Only the current one: a let-go web view that ends late never stops the next play.
        guard webView === self.webView else { return }
        release(why: "lost")
    }

    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        loaded = true
        let calls = waiting
        waiting = []
        for js in calls { webView.evaluateJavaScript(js) }
    }

    /// Nothing navigates inside it: a link in YouTube's frame (its logo) goes nowhere; it takes no input anyway.
    func webView(_ webView: WKWebView, decidePolicyFor action: WKNavigationAction) async -> WKNavigationActionPolicy {
        action.navigationType == .linkActivated ? .cancel : .allow
    }
}

/// Holds the player weakly for the content controller.
private final class WeakHandler: NSObject, WKScriptMessageHandler {
    weak var target: TrailerPlayer?
    init(_ target: TrailerPlayer) { self.target = target }

    func userContentController(_ controller: WKUserContentController, didReceive message: WKScriptMessage) {
        let body = message.body
        MainActor.assumeIsolated { target?.receive(body) }
    }
}

/// The player in the hero's frame, oversized to cover it and clipped by it; it takes no input.
struct TrailerSurface: View {
    var player: TrailerPlayer = .shared

    var body: some View {
        GeometryReader { geo in
            let size = HeroTrailers.coverSize(geo.size)
            WebHost(player: player)
                .frame(width: size.width, height: size.height)
                .position(x: geo.size.width / 2, y: geo.size.height / 2)
                .allowsHitTesting(false)
                .accessibilityHidden(true)
        }
        .clipped()
    }
}

#if os(iOS)
private struct WebHost: UIViewRepresentable {
    let player: TrailerPlayer

    func makeUIView(context: Context) -> UIView {
        let host = UIView()
        host.isUserInteractionEnabled = false
        host.backgroundColor = .clear
        attach(to: host)
        return host
    }

    func updateUIView(_ host: UIView, context: Context) { attach(to: host) }

    private func attach(to host: UIView) {
        let web = player.web
        guard web.superview !== host else { return }
        web.removeFromSuperview()
        web.frame = host.bounds
        web.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        host.addSubview(web)
    }
}
#else
private struct WebHost: NSViewRepresentable {
    let player: TrailerPlayer

    func makeNSView(context: Context) -> NSView {
        let host = PassThroughView()
        attach(to: host)
        return host
    }

    func updateNSView(_ host: NSView, context: Context) { attach(to: host) }

    private func attach(to host: NSView) {
        let web = player.web
        guard web.superview !== host else { return }
        web.removeFromSuperview()
        web.frame = host.bounds
        web.autoresizingMask = [.width, .height]
        host.addSubview(web)
    }
}

/// No hits on the Mac: clicks and scrolls go to the page behind.
private final class PassThroughView: NSView {
    override func hitTest(_ point: NSPoint) -> NSView? { nil }
}
#endif
#endif

