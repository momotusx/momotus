import type { BrowserContext, Page } from "@playwright/test";

/** A stand-in for YouTube's IFrame API: a black frame, muted only after `mute()` and a short delay as the real one
 * answers, PLAYING after `playVideo`, PAUSED after `pauseVideo`, a clock that moves while it plays, `seekTo` that
 * moves it, and `onError 150` when the page asks for one (`ytFail`, or the ids in `ytFailIds`). Every call is logged in
 * order on `window.ytLog`; the length is `window.ytDuration` or 120 s. */
export const FAKE_API = `
window.ytLog = [];
window.YT = {
  PlayerState: { PLAYING: 1, ENDED: 0 },
  Player: function (el, opts) {
    const frame = document.createElement("iframe");
    frame.srcdoc = "<body style='margin:0;background:#000'></body>";
    el.replaceWith(frame);
    let muted = false, volume = 100, t = 0, state = -1, clock = null;
    const log = (x) => window.ytLog.push(x);
    log("new " + opts.videoId + " " + opts.host);
    this.getIframe = () => frame;
    this.mute = () => { log("mute"); setTimeout(() => { muted = true; }, 30); };
    this.unMute = () => { log("unMute"); muted = false; };
    this.setVolume = (v) => { log("setVolume " + v); setTimeout(() => { volume = v; }, 30); };
    this.isMuted = () => muted;
    this.getVolume = () => volume;
    this.getDuration = () => window.ytDuration ?? 120;
    this.unloadModule = (m) => log("unload " + m);
    this.getOption = () => ({});
    this.setOption = () => {};
    this.getCurrentTime = () => t;
    this.getPlayerState = () => state;
    this.seekTo = (s, ahead) => { log("seekTo " + s + " " + ahead); t = s; };
    this.pauseVideo = () => { log("pauseVideo"); state = 2; opts.events.onStateChange({ data: 2 }); };
    this.playVideo = () => {
      log("playVideo muted=" + muted + " volume=" + volume);
      if (window.ytFail || (window.ytFailIds || []).includes(opts.videoId)) return setTimeout(() => opts.events.onError({ data: 150 }), 50);
      setTimeout(() => { state = 1; opts.events.onStateChange({ data: 1 }); }, 50);
      clock ??= setInterval(() => { if (state === 1) t += 0.25; }, 250);
    };
    this.destroy = () => { log("destroy"); clearInterval(clock); frame.remove(); };
    setTimeout(() => opts.events.onReady({ target: this }), 20);
  },
};
window.onYouTubeIframeAPIReady && window.onYouTubeIframeAPIReady();
`;

/** Every YouTube script this run asks for is the stand-in: no third-party network in a run. */
export async function fakeYouTube(target: BrowserContext | Page) {
  await target.route("https://www.youtube.com/iframe_api", (r) => r.fulfill({ contentType: "text/javascript", body: FAKE_API }));
}

/** The calls the stand-in got, in order. */
export const ytLog = (page: Page) => page.evaluate(() => (window as unknown as { ytLog?: string[] }).ytLog ?? []);
