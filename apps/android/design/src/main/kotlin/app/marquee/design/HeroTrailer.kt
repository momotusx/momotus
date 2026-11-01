package app.marquee.design

import android.annotation.SuppressLint
import android.app.ActivityManager
import android.content.ComponentCallbacks2
import android.content.Context
import android.content.res.Configuration
import android.net.ConnectivityManager
import android.os.PowerManager
import android.util.Log
import android.view.MotionEvent
import android.view.ViewGroup
import android.view.accessibility.AccessibilityManager
import android.webkit.ConsoleMessage
import android.webkit.RenderProcessGoneDetail
import android.webkit.WebChromeClient
import android.webkit.WebResourceRequest
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.FrameLayout
import androidx.compose.foundation.layout.Box
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.layout.layout
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.viewinterop.AndroidView
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.currentStateAsState
import kotlinx.coroutines.delay
import org.json.JSONObject
import kotlin.math.max
import kotlin.math.roundToInt

/**
 * The hero's muted trailer (plans/appletv-polish.md 4.2, the owner's pick of 2026-10-24): YouTube's own embedded
 * player in one WebView per Home visit, oversized to cover the hero's frame so YouTube's bars fall outside the
 * clip, under the page's art, which fades away only once YouTube's start controls have gone. It is muted and read
 * back before it plays, takes no touch or focus, and goes back to the art a second before its cap.
 */
object HeroTrailers {
    /** A walk build (`WalkMute`, read by the app): no sound reaches the player, whatever the switch says. */
    @Volatile
    var walk = false

    /** A walk that asked to see the sound switch (`--ez soundSwitch true`); it stays silent all the same. */
    @Volatile
    var walkSwitch = false

    /**
     * The viewer's sound for the hero's trailers, for this session: off at launch, and only a press of the switch
     * turns it on. A new trailer, and a switch drawn again, keep it.
     */
    var sound by mutableStateOf(false)

    /** The art above the player fades this long after YouTube says it plays (Phase 0: its controls are gone by 4 s). */
    const val REVEAL_MS = 4500L

    /** The player past the frame on every side. */
    const val OVERSCAN = 1.12f

    const val CAP_S = 45

    /** A YouTube video id as YouTube writes it, or none: it goes into the player's page. */
    fun checkedId(raw: String?): String? = raw?.takeIf { ID.matches(it) }

    private val ID = Regex("^[A-Za-z0-9_-]{11}$")
}

/** What the device and the moment allow (decision 5); every one false blocks a preview. */
data class TrailerSignals(
    val enabled: Boolean,
    /** 6 GB of memory or more, and not a low-RAM device (the player costs about 200 MB, Phase 0). */
    val capable: Boolean,
    val visible: Boolean,
    val reducedMotion: Boolean,
    val metered: Boolean,
    /** Battery saver: read for the log only, it blocks nothing (owner, 2026-10-30: the preview is muted and short). */
    val powerSave: Boolean,
    val screenReader: Boolean,
    /** The TV form shows none until a real TV is measured. */
    val tv: Boolean,
    val hasTrailer: Boolean,
)

/** What holds a preview back, by name; empty when it may play. */
fun trailerBlocks(s: TrailerSignals): List<String> =
    buildList {
        if (!s.enabled) add("previews off")
        if (!s.capable) add("memory")
        if (!s.visible) add("not visible")
        if (s.reducedMotion) add("reduced motion")
        if (s.metered) add("metered")
        if (s.screenReader) add("screen reader")
        if (s.tv) add("tv")
        if (!s.hasTrailer) add("no trailer")
    }

fun trailerAllowed(s: TrailerSignals): Boolean = trailerBlocks(s).isEmpty()

/** [trailerAllowed], logging what held it back under MarqueeTrailer, so a walk reads why nothing played. */
fun trailerAllowedLogged(s: TrailerSignals): Boolean {
    val blocks = trailerBlocks(s)
    if (blocks.isNotEmpty()) {
        Log.i(
            "MarqueeTrailer",
            "trailer held: ${blocks.joinToString()} (battery saver ${s.powerSave})",
        )
    }
    return blocks.isEmpty()
}

/** The device's own switch (never synced), default on; Settings > Playback. */
object TrailerPreviews {
    private const val PREFS = "trailer-previews"

    fun on(context: Context): Boolean = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getBoolean("on", true)

    fun set(
        context: Context,
        on: Boolean,
    ) = context
        .getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        .edit()
        .putBoolean("on", on)
        .apply()

    /** The device's side of decision 5, read when a trailer is due; [minMemory] is the memory it needs. */
    fun signals(
        context: Context,
        visible: Boolean,
        reducedMotion: Boolean,
        tv: Boolean,
        hasTrailer: Boolean,
        minMemory: Long = SIX_GB,
    ): TrailerSignals {
        val am = context.getSystemService(ActivityManager::class.java)
        val memory = ActivityManager.MemoryInfo().also { am?.getMemoryInfo(it) }
        val connectivity = context.getSystemService(ConnectivityManager::class.java)
        return TrailerSignals(
            enabled = on(context),
            capable =
                am != null &&
                    trailerCapable(memory.totalMem, memory.availMem, am.isLowRamDevice, memory.lowMemory, minMemory),
            visible = visible,
            reducedMotion = reducedMotion,
            metered = connectivity?.isActiveNetworkMetered ?: true,
            powerSave = context.getSystemService(PowerManager::class.java)?.isPowerSaveMode == true,
            screenReader =
                context
                    .getSystemService(
                        AccessibilityManager::class.java,
                    )?.isTouchExplorationEnabled == true,
            tv = tv,
            hasTrailer = hasTrailer,
        )
    }

    /** Whether a TV form's title pages may play their header trailer: its memory floor (plans/trailer-inline.md 4). */
    fun headerOnTv(context: Context): Boolean {
        val am = context.getSystemService(ActivityManager::class.java) ?: return false
        val memory = ActivityManager.MemoryInfo().also { am.getMemoryInfo(it) }
        // The device's size only: whether it has room right now is read when a trailer is due.
        return !am.isLowRamDevice && memory.totalMem >= TitleTrailers.TV_FLOOR
    }

    // totalMem reads a little under the marketed size (the kernel keeps some), so 6 GB phones read about 5.5.
    internal const val SIX_GB = 5_400_000_000L
}

/**
 * Whether a device may play a muted preview now (plans/trailer-inline.md 4): its size against [floor] (`totalMem`
 * bytes), not a low-RAM device, not under memory pressure right now, and with room for the player's cost free right
 * now. A 16 GB Pixel cannot prove a 2 GB stick, so the static floor has this runtime guard beside it.
 */
fun trailerCapable(
    totalMem: Long,
    availMem: Long,
    lowRam: Boolean,
    lowMemory: Boolean,
    floor: Long,
): Boolean = !lowRam && !lowMemory && totalMem >= floor && availMem >= TitleTrailers.ROOM

/** The end of a play whose renderer died or whose memory the system took back: shown as the quiet line, never retried. */
const val LOST = "lost"

/** A title page's header trailer (plans/trailer-inline.md 3, 4). */
object TitleTrailers {
    /** Titles whose header trailer ended, failed or was opened full screen this session: it does not play again. */
    val spent = mutableSetOf<String>()

    /**
     * The size a TV form needs for the header trailer (plans/trailer-inline.md 4a): the 4 GB class (Google TV Streamer,
     * newer Shields), so 1.5 to 3 GB sticks show the art and play Trailer full screen only. A judgment, not a margin:
     * the safety is the runtime guard beside it ([ROOM], memory pressure, a trim, a lost renderer).
     */
    const val TV_FLOOR = 3_500_000_000L

    /**
     * The memory a preview needs free when it starts: twice its cost C as measured on the Pixel in TV form on 2026-10-29
     * (app plus renderer PSS, playing minus idle, median of 3 runs: 292,277 kB, about 299 MB), rounded up to 50 MB.
     */
    const val ROOM = 600_000_000L

    const val DELAY_MS = 300L
}

/**
 * Plays a title's first trailer ([youtubeId]) muted in [state] while [playing] (the header on screen, nothing over it)
 * and the app is in front, [TitleTrailers.DELAY_MS] after it settles, when the hero's signals allow it (decision 5)
 * with the TV form's own memory floor; never twice in a session.
 */
@Composable
fun TitleHeaderTrailer(
    trailers: TrailerHost,
    state: TrailerState,
    titleId: String,
    youtubeId: String?,
    playing: Boolean,
    tv: Boolean,
) {
    val context = LocalContext.current
    val reduced = LocalReducedMotion.current
    val resumed by LocalLifecycleOwner.current.lifecycle.currentStateAsState()
    val active = resumed.isAtLeast(Lifecycle.State.RESUMED)
    val id = HeroTrailers.checkedId(youtubeId)
    LaunchedEffect(id, active, playing) {
        // Only the header's own: the full-screen trailer on the same player stops it by starting. Cut by the viewer (the
        // page scrolled or covered while it played) it does not come back this session; the app in the background is
        // not a cut.
        if (state.loading) {
            trailers.stop()
            if (!playing) TitleTrailers.spent += titleId
        }
        if (id == null || !active || !playing || titleId in TitleTrailers.spent) return@LaunchedEffect
        delay(TitleTrailers.DELAY_MS)
        val floor = if (tv) TitleTrailers.TV_FLOOR else TrailerPreviews.SIX_GB
        val signals = TrailerPreviews.signals(context, true, reduced, tv = false, hasTrailer = true, minMemory = floor)
        if (!trailerAllowedLogged(signals)) return@LaunchedEffect
        trailers.play(id, state) { why -> if (why != "stopped") TitleTrailers.spent += titleId }
    }
}

/**
 * What the hero draws from the trailer: the clock follows [fraction] while [playing]; the art fades when [shown]. A
 * full-screen trailer also reads [seconds], [length] (0 until known), [paused] and [buffering].
 */
class TrailerState {
    /** A trailer was asked for: its player is placed under the art, loading. */
    var loading by mutableStateOf(false)
        internal set
    var playing by mutableStateOf(false)
        internal set
    var shown by mutableStateOf(false)
        internal set
    var fraction by mutableFloatStateOf(0f)
        internal set

    /** Also set by the full-screen controls the moment they seek, so the bar never jumps back for a tick. */
    var seconds by mutableFloatStateOf(0f)
    var length by mutableFloatStateOf(0f)
    var paused by mutableStateOf(false)
        internal set
    var buffering by mutableStateOf(true)
        internal set
}

/**
 * Where a seek by [by] seconds from [t] lands in a [d] second trailer: never before its start, and never into its last
 * 2 s, where the player ends it (plans/trailer-inline.md 7).
 */
fun trailerSeekTarget(
    t: Float,
    d: Float,
    by: Float,
): Float = (t + by).coerceAtMost((d - 2f).coerceAtLeast(0f)).coerceAtLeast(0f)

/** Takes no touch and no focus: pager swipes, scrolls, the remote and the full-screen controls stay with Compose (decision 15). */
@SuppressLint("ViewConstructor")
private class NoTouchWebView(
    context: Context,
) : WebView(context) {
    init {
        isFocusable = false
        isFocusableInTouchMode = false
        importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS
    }

    @SuppressLint("ClickableViewAccessibility")
    override fun dispatchTouchEvent(event: MotionEvent): Boolean = false

    @SuppressLint("ClickableViewAccessibility")
    override fun onTouchEvent(event: MotionEvent): Boolean = false
}

/**
 * The one player of a Home visit, or of a title page (its header, then its full-screen trailer): created on first use,
 * reused, destroyed when the screen goes. [play] starts a trailer into [state]; [stop] ends it. Each play has its own
 * generation, so a late message of the one before (its `end stopped`) never reaches the next (review r1-A 1).
 */
class TrailerHost internal constructor(
    private val context: Context,
    /**
     * Incognito: no trailer at all (plans/batch-10-incognito.md 5.5), since YouTube's player keeps its cookies and
     * cache in the app's WebView profile. [play] does nothing, so no WebView is ever made and the art stays.
     */
    val off: Boolean = false,
) {
    private var web: NoTouchWebView? = null
    private var state: TrailerState? = null
    private var onEnd: ((String) -> Unit)? = null
    private var generation = 0
    private val main = android.os.Handler(android.os.Looper.getMainLooper())
    private val reveal = Runnable { state?.shown = true }

    /**
     * The player's page has loaded: until then calls wait, in order, since a call that reached the page before its
     * script would run nothing, or worse, the browser's own `stop()`, which cancels the page's load.
     */
    private var loaded = false
    private val waiting = mutableListOf<String>()

    /** Before the page loads, calls wait in order: a press of Sound while it loads must not drop the play itself. */
    private fun run(js: String) {
        val view = web
        if (loaded && view != null) view.evaluateJavascript(js, null) else waiting += js
    }

    @SuppressLint("SetJavaScriptEnabled")
    internal fun view(): WebView =
        web ?: NoTouchWebView(context).also { view ->
            view.setBackgroundColor(android.graphics.Color.TRANSPARENT)
            view.settings.javaScriptEnabled = true
            view.settings.mediaPlaybackRequiresUserGesture = false
            view.settings.allowFileAccess = false
            view.settings.allowContentAccess = false
            view.webChromeClient =
                object : WebChromeClient() {
                    override fun onConsoleMessage(message: ConsoleMessage): Boolean {
                        // Only the page this app wrote speaks to it.
                        if (message.sourceId().startsWith(BASE)) {
                            message
                                .message()
                                .removePrefix("MQT ")
                                .takeIf { it != message.message() }
                                ?.let(::onMessage)
                        }
                        return true
                    }
                }
            view.webViewClient =
                object : WebViewClient() {
                    // Nothing navigates inside it; a tap never reaches it anyway.
                    override fun shouldOverrideUrlLoading(
                        view: WebView,
                        request: WebResourceRequest,
                    ): Boolean = request.isForMainFrame

                    override fun onPageFinished(
                        view: WebView,
                        url: String?,
                    ) {
                        loaded = true
                        val calls = waiting.toList()
                        waiting.clear()
                        calls.forEach { view.evaluateJavascript(it, null) }
                    }

                    // The renderer died (memory on a small TV): whoever plays hears it stopped, the next play makes a
                    // fresh one.
                    override fun onRenderProcessGone(
                        view: WebView,
                        detail: RenderProcessGoneDetail,
                    ): Boolean {
                        main.post { destroy(LOST) }
                        return true
                    }
                }
            view.loadDataWithBaseURL(BASE, PAGE, "text/html", "utf-8", null)
            web = view
        }

    /**
     * Plays [id] into [state], muted, capped at [cap] seconds (0: its own length, still ended a second early). [full]: the
     * full-screen trailer, its time read every 250 ms, its sound turned on once it plays when [loud] (never in a walk).
     */
    fun play(
        id: String,
        state: TrailerState,
        cap: Int = HeroTrailers.CAP_S,
        full: Boolean = false,
        loud: Boolean = false,
        onEnd: (String) -> Unit,
    ) {
        if (off) return
        stop()
        val checked = HeroTrailers.checkedId(id) ?: return onEnd("error")
        this.state = state
        this.onEnd = onEnd
        generation += 1
        state.loading = true
        state.buffering = true
        state.paused = false
        state.playing = false
        state.shown = false
        state.fraction = 0f
        state.seconds = 0f
        state.length = 0f
        view()
        run("play('$checked', $cap, $generation, $full, ${loud && !HeroTrailers.walk})")
    }

    /** Pauses a playing trailer, plays a paused one. */
    fun toggle() {
        if (state != null) run("toggle()")
    }

    /** To [seconds] in the trailer. */
    fun seek(seconds: Float) {
        if (state != null) run("seek($seconds)")
    }

    /** The full-screen trailer's own sound, never the session's ([HeroTrailers.sound]); never in a walk. */
    fun loud(on: Boolean) {
        if (state != null && !HeroTrailers.walk) run(if (on) "sound(true)" else "sound(false)")
    }

    fun stop() {
        main.removeCallbacks(reveal)
        state?.let {
            it.shown = false
            it.playing = false
            it.loading = false
        }
        state = null
        onEnd = null
        if (web != null) run("halt()")
    }

    /** The switch was pressed: sound for this session ([HeroTrailers.sound]); never in a walk. */
    fun sound(on: Boolean) {
        HeroTrailers.sound = on
        if (HeroTrailers.walk) return
        if (web != null) run(if (on) "sound(true)" else "sound(false)")
    }

    /**
     * Lets the web view and its renderer go; the next play makes a fresh one. Whoever plays hears [why]: `stopped` (the
     * app in the background: a full-screen trailer closes rather than stay black, review r1-A 5) or [LOST] (the
     * renderer died, or the system asked for memory back: the full screen says it cannot play, a header does not play
     * again this session).
     */
    fun destroy(why: String = "stopped") {
        val done = onEnd
        stop()
        done?.invoke(why)
        web?.let {
            (it.parent as? ViewGroup)?.removeView(it)
            it.destroy()
        }
        web = null
        loaded = false
        waiting.clear()
    }

    private fun onMessage(json: String) {
        val m = runCatching { JSONObject(json) }.getOrNull() ?: return
        if (m.optInt("gen") != generation) return
        val s = state ?: return
        val event = m.optString("event")
        // A walk's evidence that a trailer played (plans/appletv-polish.md 9): its state, never its id.
        if (event == "seek") {
            Log.i("MarqueeTrailer", "trailer seek from ${m.optInt("from")} to ${m.optInt("to")}")
        } else if (event != "time") {
            Log.i("MarqueeTrailer", "trailer $event ${m.optString("why")}".trim())
        }
        when (event) {
            "playing" -> {
                s.playing = true
                s.buffering = false
                main.postDelayed(reveal, HeroTrailers.REVEAL_MS)
                if (!m.optBoolean("full") && HeroTrailers.sound &&
                    !HeroTrailers.walk
                ) {
                    web?.evaluateJavascript("sound(true)", null)
                }
            }

            "time" -> {
                s.fraction = m.optDouble("fraction", 0.0).toFloat()
                s.seconds = m.optDouble("t", 0.0).toFloat()
                s.length = m.optDouble("d", 0.0).toFloat()
                val phase = m.optString("state")
                s.paused = phase == "paused"
                s.buffering = phase == "buffering"
            }

            "end" -> {
                val why = m.optString("why")
                val done = onEnd
                main.removeCallbacks(reveal)
                s.shown = false
                s.playing = false
                s.loading = false
                state = null
                onEnd = null
                done?.invoke(why)
            }
        }
    }

    private companion object {
        /** The app's own id as the page's address: YouTube reads it as the embedding app (its Referer rule). */
        const val BASE = "https://app.momotus.android/"

        /**
         * The player's page: the official IFrame API, muted and read back before it plays (decision 6). The same script
         * as the Apple app's (HeroTrailer.swift), held to one behaviour by e2e/trailer-page.spec.ts.
         */
        val PAGE =
            """
            <!doctype html><html><head><meta name="viewport" content="width=device-width, initial-scale=1">
            <style>html,body{margin:0;height:100%;background:transparent;overflow:hidden}
            #p{position:fixed;inset:0;pointer-events:none}
            #p>div{position:absolute;inset:0;pointer-events:none}
            #p iframe{width:100%;height:100%;border:0;display:block;pointer-events:none}</style></head>
            <body><div id="p"></div><script>
            let player=null, wrap=null, timers=[], ended=true, playing=false, poll=null, api=null, gen=0, full=false, want=false;
            const say=(o)=>console.log("MQT "+JSON.stringify(Object.assign({gen,full},o)));
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
                        captionsOff(); if(len>0&&t>=len-1) finish(cap?"cap":"ended"); }, f?250:1000); }
                    if(e.data===0) finish(cap?"cap":"ended"); },
                  onError:(e)=>{ if(gen===g){ say({event:"refused",why:"youtube "+e.data}); finish("error"); } } } }); }
            </script></body></html>
            """.trimIndent()
    }
}

/** One player per Home visit (the lead, 2026-10-24: never created per scroll); it goes when Home goes. */
@Composable
fun rememberTrailerHost(): TrailerHost {
    val context = LocalContext.current
    val off = LocalIncognito.current
    val host = remember(context, off) { TrailerHost(context, off) }
    // In the background the WebView's renderer goes too (review r1, L2); the next play makes a fresh one.
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    DisposableEffect(host, lifecycle) {
        val stopped = LifecycleEventObserver { _, event -> if (event == Lifecycle.Event.ON_STOP) host.destroy() }
        lifecycle.addObserver(stopped)
        // The system asks for memory back while the app is in front: the player goes, whoever plays hears it lost.
        val trim =
            object : ComponentCallbacks2 {
                override fun onTrimMemory(level: Int) {
                    @Suppress("DEPRECATION")
                    if (level >= ComponentCallbacks2.TRIM_MEMORY_RUNNING_LOW &&
                        level < ComponentCallbacks2.TRIM_MEMORY_UI_HIDDEN
                    ) {
                        host.destroy(LOST)
                    }
                }

                override fun onConfigurationChanged(newConfig: Configuration) = Unit

                @Deprecated("Deprecated in Java")
                override fun onLowMemory() = host.destroy(LOST)
            }
        context.applicationContext.registerComponentCallbacks(trim)
        onDispose {
            lifecycle.removeObserver(stopped)
            context.applicationContext.unregisterComponentCallbacks(trim)
            host.destroy()
        }
    }
    return host
}

/**
 * The player in the hero's frame, under its art: one WebView, attached here and detached (never destroyed) when
 * this leaves, oversized to cover the frame by [HeroTrailers.OVERSCAN] and clipped by it.
 */
@Composable
fun TrailerSurface(
    host: TrailerHost,
    modifier: Modifier = Modifier,
) {
    Box(modifier.clipToBounds()) {
        AndroidView(
            factory = { context ->
                // A surface that leaves can release after the next one attaches (a hero page, the title header under the
                // full-screen trailer): each holds the view in a frame of its own and lets go only of what it still holds.
                FrameLayout(context).apply {
                    addView(host.view().also { (it.parent as? ViewGroup)?.removeView(it) })
                }
            },
            onRelease = { it.removeAllViews() },
            modifier =
                Modifier.layout { measurable, constraints ->
                    val w = constraints.maxWidth
                    val h = constraints.maxHeight
                    val ph = max(h.toFloat(), w * 9f / 16f) * HeroTrailers.OVERSCAN
                    val pw = ph * 16f / 9f
                    val placeable = measurable.measure(Constraints.fixed(pw.roundToInt(), ph.roundToInt()))
                    layout(w, h) { placeable.place(((w - pw) / 2).roundToInt(), ((h - ph) / 2).roundToInt()) }
                },
        )
    }
}
