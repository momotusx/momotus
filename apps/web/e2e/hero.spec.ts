/** Home's hero carousel and its trailer (plans/appletv-polish.md 4.1, 4.2, 8): pages and dots from the
 * core's `heroes`, the timer on a fake clock, a page by hand that stops it, Right and Left paging on
 * TV, and a muted trailer from a fake YouTube player (no test reaches youtube.com): muted before it
 * plays, covering the hero's frame under the art until its controls fade, taking no input, and gone
 * on an error. */

import { type BrowserContext, expect, type Page, test } from "@playwright/test";

const activePage = (page: Page) => page.evaluate(() => Number(document.querySelector<HTMLElement>(".hero-page.on")?.dataset.page ?? -1));
const activeDot = (page: Page) => page.evaluate(() => [...document.querySelectorAll(".hero-dots .dot")].findIndex((d) => d.classList.contains("on")));

async function home(context: BrowserContext, form: "phone" | "desktop" | "tv") {
  const page = await context.newPage();
  await page.clock.install();
  await page.goto(`/?form=${form}`);
  await expect(page.locator(".hero-page.on .reason")).toBeVisible({ timeout: 30_000 });
  await expect.poll(() => page.locator(".hero-page").count(), { timeout: 30_000 }).toBeGreaterThan(2);
  // From here time moves only when the test says; a pointer move starts the dwell over.
  await page.clock.pauseAt(await page.evaluate(() => Date.now() + 50));
  await page.mouse.move(1, (page.viewportSize()?.height ?? 900) - 1);
  return page;
}

for (const form of ["phone", "desktop", "tv"] as const) {
  test(`${form}: the timer pages on, the pill fills, a page by hand stops it`, async ({ browser }) => {
    const size = form === "phone" ? { width: 390, height: 844 } : { width: 1440, height: 900 };
    const context = await browser.newContext({ viewport: form === "tv" ? { width: 1920, height: 1080 } : size });
    const page = await home(context, form);
    const pages = await page.locator(".hero-page").count();
    expect(await page.locator(".hero-dots .dot").count()).toBe(pages);
    const base = await activePage(page);
    // Half the dwell: the pill is half full and nothing moved.
    await page.clock.runFor(4000);
    expect(await activePage(page)).toBe(base);
    const fill = await page.locator(".hero-dots .dot.on i").evaluate((el) => Number(getComputedStyle(el).getPropertyValue("--fill") || (el as HTMLElement).style.getPropertyValue("--fill")));
    expect(fill).toBeGreaterThan(0.4);
    expect(fill).toBeLessThan(0.6);
    await page.clock.runFor(5000);
    await expect.poll(() => activePage(page)).toBe((base + 1) % pages);
    expect(await activeDot(page)).toBe((base + 1) % pages);
    // Only the active page carries Play's key.
    expect(await page.locator('[data-f="hero:play"], [data-f="hero:details"]').evaluateAll((els) => els.every((el) => el.closest(".hero-page.on")))).toBe(true);
    if (form !== "tv") {
      // A dot by hand: that page, and the timer stops for the session.
      const target = ((await activePage(page)) + 2) % pages;
      await page.locator(".hero-dots .dot").nth(target).click();
      await expect.poll(() => activePage(page)).toBe(target);
      await page.mouse.move(1, size.height - 1);
      await page.clock.runFor(30_000);
      expect(await activePage(page)).toBe(target);
    }
    await context.close();
  });
}

test("reduced motion: the pages never move by themselves", async ({ browser }) => {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, reducedMotion: "reduce" });
  const page = await home(context, "desktop");
  const base = await activePage(page);
  await page.clock.runFor(30_000);
  expect(await activePage(page)).toBe(base);
  await context.close();
});

test("TV: Right from the last button pages on with the focus on the same button, Left from the first comes back", async ({ browser }) => {
  const context = await browser.newContext({ viewport: { width: 1920, height: 1080 }, reducedMotion: "reduce" });
  const page = await home(context, "tv");
  await page.locator('.hero-page.on [data-f="hero:details"]').focus();
  await page.keyboard.press("ArrowRight");
  await expect.poll(() => activePage(page)).toBe(1);
  await expect.poll(() => page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f)).toBe("hero:details");
  expect(await page.evaluate(() => !!document.activeElement?.closest(".hero-page.on"))).toBe(true);
  const first = page.locator(".hero-page.on .acts [data-f]").first();
  await first.focus();
  await page.keyboard.press("ArrowLeft");
  await expect.poll(() => activePage(page)).toBe(0);
  await context.close();
});

/** A stand-in for YouTube's IFrame API: a blank frame, muted only after `mute()` and a short delay as
 * the real one answers, PLAYING after `playVideo`, or `error` when the page asks for one. Every call
 * is logged in order on `window.ytLog`. */
const FAKE_API = `
window.ytLog = [];
window.YT = {
  PlayerState: { PLAYING: 1, ENDED: 0 },
  Player: function (el, opts) {
    const frame = document.createElement("iframe");
    frame.src = "about:blank";
    el.replaceWith(frame);
    let muted = false, volume = 100, t = 0, playing = false;
    const log = (x) => window.ytLog.push(x);
    log("new " + opts.videoId + " " + opts.host);
    this.getIframe = () => frame;
    this.mute = () => { log("mute"); setTimeout(() => { muted = true; }, 30); };
    this.unMute = () => { log("unMute"); muted = false; };
    this.setVolume = (v) => { log("setVolume " + v); setTimeout(() => { volume = v; }, 30); };
    this.isMuted = () => muted;
    this.getVolume = () => volume;
    this.getDuration = () => 120;
    this.unloadModule = (m) => log("unload " + m);
    this.getOption = () => ({});
    this.setOption = () => {};
    this.getCurrentTime = () => t;
    this.playVideo = () => {
      log("playVideo muted=" + muted + " volume=" + volume);
      if (window.ytFail) return setTimeout(() => opts.events.onError({ data: 150 }), 50);
      setTimeout(() => { playing = true; opts.events.onStateChange({ data: 1 }); }, 50);
      setInterval(() => { if (playing) t += 0.25; }, 250);
    };
    this.destroy = () => { log("destroy"); frame.remove(); };
    setTimeout(() => opts.events.onReady({ target: this }), 20);
  },
};
window.onYouTubeIframeAPIReady && window.onYouTubeIframeAPIReady();
`;

async function trailerPage(fail: boolean) {
  return async ({ browser }: { browser: import("@playwright/test").Browser }) => {
    const context = await browser.newContext({ viewport: { width: 390, height: 844 } });
    await context.route("https://www.youtube.com/iframe_api", (r) => r.fulfill({ contentType: "text/javascript", body: FAKE_API }));
    await context.addInitScript((f) => {
      localStorage.setItem("mq.trailerPreviews", "e2e");
      (window as unknown as { ytFail: boolean }).ytFail = f;
    }, fail);
    return { context, page: await home(context, "phone") };
  };
}

test("phone: the trailer plays muted, after the read-back, full bleed under the art until its controls fade, taking no input", async ({ browser }) => {
  const { context, page } = await (await trailerPage(false))({ browser });
  await page.clock.runFor(1500);
  await expect.poll(() => page.evaluate(() => (window as unknown as { ytLog: string[] }).ytLog.some((l) => l.startsWith("playVideo"))), { timeout: 10_000 }).toBe(true);
  const log = await page.evaluate(() => (window as unknown as { ytLog: string[] }).ytLog);
  expect(log[0]).toMatch(/^new [A-Za-z0-9_-]{11} https:\/\/www\.youtube-nocookie\.com$/);
  // Never played before it read muted with no volume.
  const play = log.findIndex((l) => l.startsWith("playVideo"));
  expect(play).toBeGreaterThan(log.indexOf("mute"));
  expect(log[play]).toBe("playVideo muted=true volume=0");
  // Playing, but still under the art: YouTube's start controls have not faded yet.
  await page.clock.runFor(2000);
  await expect(page.locator(".hero.trailer-on")).toHaveCount(0);
  // The player covers the whole frame past every edge.
  const fit = await page.evaluate(() => {
    const f = document.querySelector(".hero")!.getBoundingClientRect();
    const p = document.querySelector(".hero-player")!.getBoundingClientRect();
    return { left: p.left < f.left, right: p.right > f.right, top: p.top < f.top, bottom: p.bottom > f.bottom, ratio: p.width / p.height };
  });
  expect(fit).toEqual({ left: true, right: true, top: true, bottom: true, ratio: expect.closeTo(16 / 9, 2) });
  // 4.5 s after it plays, the art fades away above it.
  await page.clock.runFor(3000);
  await expect(page.locator(".hero.trailer-on")).toHaveCount(1, { timeout: 10_000 });
  // It takes no input.
  const frame = page.locator(".hero-player iframe");
  expect(await frame.getAttribute("tabindex")).toBe("-1");
  expect(await frame.evaluate((el) => (el as HTMLIFrameElement).inert)).toBe(true);
  await page.keyboard.press("Tab");
  expect(await page.evaluate(() => document.activeElement?.tagName)).not.toBe("IFRAME");
  // The sound switch is never drawn in a walk or a test run.
  expect(await page.locator(".trailer-mute").count()).toBe(0);
  // A page by hand stops it at once and the art comes back.
  await page.locator(".hero-dots .dot").nth(1).click();
  await expect(page.locator(".hero.trailer-on")).toHaveCount(0);
  expect(await page.evaluate(() => (window as unknown as { ytLog: string[] }).ytLog.at(-1))).toBe("destroy");
  await context.close();
});

test("phone: a trailer YouTube refuses leaves the art as it was", async ({ browser }) => {
  const { context, page } = await (await trailerPage(true))({ browser });
  await page.clock.runFor(2000);
  await expect.poll(() => page.evaluate(() => (window as unknown as { ytLog: string[] }).ytLog.includes("destroy")), { timeout: 10_000 }).toBe(true);
  await expect(page.locator(".hero.trailer-on")).toHaveCount(0);
  await expect(page.locator(".hero-page.on .hero-bd img").first()).toHaveCSS("opacity", "1");
  await context.close();
});
