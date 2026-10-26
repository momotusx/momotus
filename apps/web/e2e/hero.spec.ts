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
  // The pages grow again once every row has answered: the count holds for a second first, so no test counts a
  // list that is still growing.
  for (let n = -1, k = 0; k < 30; k++) {
    const count = await page.locator(".hero-page").count();
    if (count === n) break;
    n = count;
    await page.waitForTimeout(1000);
  }
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
    // Filled dots, never the status dots' ring (app.css .dot).
    expect(await page.locator(".hero-dots .dot").evaluateAll((els) => els.map((el) => getComputedStyle(el).boxShadow))).toEqual(Array(pages).fill("none"));
    // A dot by hand is round inside its hit area, never its padded box's square corners.
    expect(
      await page.locator(".hero-dots button.dot").evaluateAll((els) =>
        els.every((el) => getComputedStyle(el).backgroundColor === "rgba(0, 0, 0, 0)" && getComputedStyle(el, "::before").borderRadius === "999px"),
      ),
    ).toBe(true);
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
    // The timer's page is not said: the status line stays empty (review r2, R2-3).
    await expect(page.locator(".hero-said")).toHaveText("");
    // Only the active page carries Play's key.
    expect(await page.locator('[data-f="hero:play"], [data-f="hero:details"]').evaluateAll((els) => els.every((el) => el.closest(".hero-page.on")))).toBe(true);
    if (form !== "tv") {
      // A dot by hand: that page, and the timer stops for the session.
      const target = ((await activePage(page)) + 2) % pages;
      await page.locator(".hero-dots .dot").nth(target).click();
      await expect.poll(() => activePage(page)).toBe(target);
      // A page chosen by hand is said, as the slide's own label reads.
      const label = await page.locator(".hero-page").nth(target).getAttribute("aria-label");
      await expect(page.locator('.hero [role="status"]')).toHaveText(label ?? "");
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
  // Which button is first waits on the core's Play: a Play landing between the focus and the key would make
  // Left a plain move to it.
  await expect(page.locator(".hero-page.on .acts .slot")).toHaveCount(0);
  const first = page.locator(".hero-page.on .acts [data-f]").first();
  await first.focus();
  await page.keyboard.press("ArrowLeft");
  await expect.poll(() => activePage(page)).toBe(0);
  await context.close();
});

test("TV: the timer pages with the focus on a hero button, which goes with the page; a key holds the pages 5 s", async ({ browser }) => {
  // The owner, 2026-10-26: the TV hero slides while focus rests on it.
  const context = await browser.newContext({ viewport: { width: 1920, height: 1080 } });
  const page = await home(context, "tv");
  const pages = await page.locator(".hero-page").count();
  const key = await page.evaluate(() => (document.querySelector('.hero-page.on [data-f="hero:play"]') ? "hero:play" : "hero:details"));
  await page.locator(`.hero-page.on [data-f="${key}"]`).focus();
  const base = await activePage(page);
  await page.clock.runFor(9000);
  await expect.poll(() => activePage(page)).toBe((base + 1) % pages);
  // The same button on the new page, else its first.
  const want = await page.evaluate(
    (k) => (document.querySelector(`.hero-page.on [data-f="${k}"]`) ? k : (document.querySelector(".hero-page.on .acts [data-f]") as HTMLElement | null)?.dataset.f),
    key,
  );
  const now = await page.evaluate(() => {
    const el = document.activeElement as HTMLElement | null;
    return { f: el?.dataset.f, on: !!el?.closest(".hero-page.on") };
  });
  expect(now).toEqual({ f: want, on: true });
  // A key: the pages hold until 5 s after it, then a whole dwell.
  await page.keyboard.press("Shift");
  const held = await activePage(page);
  await page.clock.runFor(10_000);
  expect(await activePage(page)).toBe(held);
  await page.clock.runFor(4_000);
  await expect.poll(() => activePage(page)).toBe((held + 1) % pages);
  await context.close();
});

test("TV: Up from the first row during a timed slide lands on the hero", async ({ browser }) => {
  // Review r3, L1: the track scrolls during the 650 ms, the old page inert and the new one part off screen.
  const context = await browser.newContext({ viewport: { width: 1920, height: 1080 } });
  const page = await home(context, "tv");
  await page.locator(".row [data-f]").first().focus();
  const base = await activePage(page);
  // 200 ms into the timer's slide.
  await page.clock.runFor(8_200);
  expect(await activePage(page)).not.toBe(base);
  await page.keyboard.press("ArrowUp");
  await expect.poll(() => page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f ?? "")).toMatch(/^hero:/);
  expect(await page.evaluate(() => !!document.activeElement?.closest(".hero-page.on"))).toBe(true);
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
  // The sound switch (the owner, 2026-10-26): at the hero's bottom right, its middle level with the dots', muted
  // until a press. A run that asked for trailers draws it; the fake player is all it reaches.
  const mute = page.locator(".trailer-mute");
  await expect(mute).toHaveAttribute("aria-label", "Trailer sound");
  await expect(mute).toHaveAttribute("aria-pressed", "false");
  expect(await switchPlace(page)).toEqual({ right: 20, level: 0 });
  await mute.click();
  await expect(mute).toHaveAttribute("aria-pressed", "true");
  expect(await ytLog(page)).toContain("unMute");
  // A page by hand stops it at once, the art comes back and the switch goes with the trailer.
  await page.locator(".hero-dots .dot").nth(1).click();
  await expect(page.locator(".hero.trailer-on")).toHaveCount(0);
  expect((await ytLog(page)).at(-1)).toBe("destroy");
  await expect(mute).toHaveCount(0);
  // The next page's trailer keeps the session's choice: sound on as soon as it plays, the switch pressed.
  const before = (await ytLog(page)).length;
  for (let i = 2; i < (await page.locator(".hero-dots .dot").count()) && !(await ytLog(page)).slice(before).some((l) => l.startsWith("new")); i++) {
    await page.locator(".hero-dots .dot").nth(i).click();
    await page.clock.runFor(1500);
  }
  await expect.poll(async () => (await ytLog(page)).slice(before).filter((l) => l.startsWith("new") || l === "unMute"), { timeout: 10_000 }).toEqual([expect.stringMatching(/^new /), "unMute"]);
  await expect(mute).toHaveAttribute("aria-pressed", "true");
  await context.close();
});

const ytLog = (page: Page) => page.evaluate(() => (window as unknown as { ytLog: string[] }).ytLog);

/** The switch's gap to the hero's right edge, and its middle's offset from the dots' middle, in whole px. */
const switchPlace = (page: Page) =>
  page.evaluate(() => {
    const f = document.querySelector(".hero")!.getBoundingClientRect();
    const m = document.querySelector(".trailer-mute")!.getBoundingClientRect();
    const d = document.querySelector(".hero-dots")!.getBoundingClientRect();
    // + 0: a round half a pixel under is -0.
    return { right: Math.round(f.right - m.right) + 0, level: Math.round((m.top + m.bottom) / 2 - (d.top + d.bottom) / 2) + 0 };
  });

test("desktop: the sound switch sits at the hero's bottom right, as tall as the dots' platter and level with it", async ({ browser }) => {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  await context.route("https://www.youtube.com/iframe_api", (r) => r.fulfill({ contentType: "text/javascript", body: FAKE_API }));
  await context.addInitScript(() => localStorage.setItem("mq.trailerPreviews", "e2e"));
  const page = await home(context, "desktop");
  await page.clock.runFor(1500);
  const mute = page.locator(".trailer-mute");
  await expect(mute).toBeVisible({ timeout: 10_000 });
  const px = await page.evaluate(() => Number.parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--px")) || 1);
  const place = await switchPlace(page);
  expect(place.level).toBe(0);
  expect(place.right).toBe(Math.round(40 * px));
  const tall = await page.evaluate(() => [".trailer-mute", ".hero-dots"].map((s) => Math.round(document.querySelector(s)!.getBoundingClientRect().height)));
  expect(tall[0]).toBe(tall[1]);
  // Off the hero there is none: the top bar no longer carries it.
  expect(await page.locator(".topnav .trailer-mute").count()).toBe(0);
  await context.close();
});

/** The first series page: its index, its title id and its Play. */
async function seriesPage(page: Page) {
  const pages = page.locator(".hero-page");
  const n = await pages.count();
  for (let i = 0; i < n; i++) {
    const p = pages.nth(i);
    if (!(await p.locator(".genre").innerText()).startsWith("Series")) continue;
    const details = (await p.locator(".acts a").last().getAttribute("href")) ?? "";
    return { i, id: decodeURIComponent(details.split("/").at(-1) ?? ""), play: p.locator(".acts a.btn.primary") };
  }
  throw new Error("no series page in the hero");
}

test("desktop: a series page plays the core's episode, its label as the title page's", async ({ browser }) => {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  const page = await home(context, "desktop");
  const { i, id, play } = await seriesPage(page);
  // "Play S1:E1" never watched, "Resume S2:E3" part way, "Play again" at the end.
  await expect(play).toHaveText(/^((Play|Resume) S\d+:E\d+|Play again)$/, { timeout: 10_000 });
  const href = (await play.getAttribute("href")) ?? "";
  expect(href).toMatch(new RegExp(`^/play/series/${id}/${id}:\\d+:\\d+$`));
  await page.locator(".hero-dots .dot").nth(i).click();
  await expect.poll(() => activePage(page)).toBe(i);
  await page.locator(".hero-page.on .acts a.btn.primary").click();
  await expect.poll(() => new URL(page.url()).pathname).toBe(href);
  await context.close();
});

/** Holds every series page's Play as the core sends it while its meta is read (`play` null, `playPending`), until
 * `release` sends the core's own last header. */
async function holdSeriesPlay(context: BrowserContext) {
  const hold = { held: true, sub: -1, header: null as unknown, release: () => {} };
  await context.routeWebSocket(/\/ws$/, (ws) => {
    const server = ws.connectToServer();
    ws.onMessage((m) => server.send(m));
    server.onMessage((m) => {
      const text = String(m);
      if (!hold.held || !text.includes('"heroes"')) return ws.send(text);
      type Header = { heroes: { tile: { kind: string }; play: unknown; playPending: boolean }[] };
      const msg = JSON.parse(text);
      const headers: Header[] = [];
      const snap = msg.result?.snapshot;
      if (snap?.header) {
        hold.sub = msg.result.sub;
        headers.push(snap.header);
      }
      for (const u of msg.updates ?? []) if (u.kind === "header") headers.push(u.header);
      for (const header of headers) {
        hold.header = structuredClone(header);
        for (const h of header.heroes) if (h.tile.kind !== "movie") Object.assign(h, { play: null, playPending: true });
      }
      ws.send(JSON.stringify(msg));
    });
    hold.release = () => {
      hold.held = false;
      ws.send(JSON.stringify({ sub: hold.sub, updates: [{ kind: "header", header: hold.header }] }));
    };
  });
  return hold;
}

for (const form of ["phone", "desktop"] as const) {
  test(`${form}: a late Play lands in its kept place, Details never moving`, async ({ browser }) => {
    const context = await browser.newContext({ viewport: form === "phone" ? { width: 390, height: 844 } : { width: 1440, height: 900 } });
    const hold = await holdSeriesPlay(context);
    const page = await home(context, form);
    const { i } = await seriesPage(page);
    await page.locator(".hero-dots .dot").nth(i).click();
    await expect.poll(() => activePage(page)).toBe(i);
    const on = page.locator(".hero-page.on");
    // While the episodes are read: Play's place is kept, hidden, out of focus and of the accessibility tree.
    const slot = on.locator(".acts .slot");
    await expect(slot).toHaveCount(1);
    expect(await slot.evaluate((el) => [getComputedStyle(el).visibility, el.getAttribute("aria-hidden"), el.tabIndex])).toEqual(["hidden", "true", -1]);
    await expect(on.locator('[data-f="hero:play"]')).toHaveCount(0);
    const before = await on.locator('[data-f="hero:details"]').boundingBox();
    hold.release();
    await expect(on.locator('[data-f="hero:play"]')).toHaveText(/^((Play|Resume) S\d+:E\d+|Play again)$/);
    await expect(slot).toHaveCount(0);
    expect(await on.locator('[data-f="hero:details"]').boundingBox()).toEqual(before);
    await context.close();
  });
}

/** Sends the core's last header again with every page's Play saying `label`. */
async function relabel(context: BrowserContext) {
  type Header = { heroes: { play: { label: string } | null }[] };
  const r = { sub: -1, header: null as Header | null, send: (_label: string) => {} };
  await context.routeWebSocket(/\/ws$/, (ws) => {
    const server = ws.connectToServer();
    ws.onMessage((m) => server.send(m));
    server.onMessage((m) => {
      const text = String(m);
      if (text.includes('"heroes"')) {
        const msg = JSON.parse(text);
        if (msg.result?.snapshot?.header) {
          r.sub = msg.result.sub;
          r.header = msg.result.snapshot.header;
        }
        for (const u of msg.updates ?? []) if (u.kind === "header") r.header = u.header;
      }
      ws.send(text);
    });
    r.send = (label) => {
      const header = structuredClone(r.header) as Header;
      for (const h of header.heroes) if (h.play) h.play.label = label;
      ws.send(JSON.stringify({ sub: r.sub, updates: [{ kind: "header", header }] }));
    };
  });
  return r;
}

for (const form of ["desktop", "tv"] as const) {
  test(`${form}: Play is as wide as its longest words, so Details never moves whatever it says`, async ({ browser }) => {
    const context = await browser.newContext({ viewport: form === "tv" ? { width: 1920, height: 1080 } : { width: 1440, height: 900 } });
    const r = await relabel(context);
    const page = await home(context, form);
    const on = page.locator(".hero-page.on");
    await expect(on.locator('[data-f="hero:play"]')).toHaveCount(1, { timeout: 10_000 });
    const boxes: ({ x: number; y: number; width: number; height: number } | null)[] = [];
    for (const label of ["Play", "Play S1:E1", "Resume S12:E10", "Play again"]) {
      r.send(label);
      await expect(on.locator('[data-f="hero:play"]')).toHaveText(label);
      boxes.push(await on.locator('[data-f="hero:details"]').boundingBox());
    }
    expect(boxes.every((b) => JSON.stringify(b) === JSON.stringify(boxes[0]))).toBe(true);
    await context.close();
  });
}

test("phone: a film played mutes the trailers again; Home after it is never loud", async ({ browser }) => {
  const { context, page } = await (await trailerPage(false))({ browser });
  await page.clock.runFor(1500);
  await expect.poll(async () => (await ytLog(page)).some((l) => l.startsWith("playVideo")), { timeout: 10_000 }).toBe(true);
  const mute = page.locator(".trailer-mute");
  await mute.click();
  await expect(mute).toHaveAttribute("aria-pressed", "true");
  // A play opens, then Back to Home.
  await page.locator(".hero-page.on .acts a.btn.primary").click();
  await expect.poll(() => new URL(page.url()).pathname).toMatch(/^\/play\//);
  await page.goBack();
  await expect(page.locator(".hero-page.on .reason")).toBeVisible({ timeout: 30_000 });
  // The next trailer plays muted, the switch unpressed.
  const before = (await ytLog(page)).length;
  for (let i = 0; i < (await page.locator(".hero-dots .dot").count()) && !(await ytLog(page)).slice(before).some((l) => l.startsWith("playVideo")); i++) {
    await page.locator(".hero-dots .dot").nth(i).click();
    await page.clock.runFor(1500);
  }
  await expect.poll(async () => (await ytLog(page)).slice(before).some((l) => l.startsWith("playVideo")), { timeout: 10_000 }).toBe(true);
  expect((await ytLog(page)).slice(before)).not.toContain("unMute");
  await expect(mute).toHaveAttribute("aria-pressed", "false");
  await context.close();
});

test("TV: a series page the timer brings lands the focus on its Play", async ({ browser }) => {
  const context = await browser.newContext({ viewport: { width: 1920, height: 1080 } });
  const page = await home(context, "tv");
  const { i, play } = await seriesPage(page);
  await expect(play).toHaveText(/ S\d+:E\d+$/, { timeout: 10_000 });
  const focused = () => page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f ?? "");
  await expect.poll(focused).toBe("hero:play");
  for (let k = 0; k < 10 && (await activePage(page)) !== i; k++) await page.clock.runFor(9000);
  expect(await activePage(page)).toBe(i);
  await expect.poll(focused).toBe("hero:play");
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

test("desktop: the owner's B2, a page stays 8 s, slides in 650 ms, its art lagging at 0.85", async ({ browser }) => {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  const page = await home(context, "desktop");
  const base = await activePage(page);
  const scroll = () => page.evaluate(() => document.querySelector(".hero-track")!.scrollLeft);
  const width = await page.evaluate(() => document.querySelector(".hero-track")!.clientWidth);
  const start = await scroll();
  // Not before its 8 s.
  await page.clock.runFor(7800);
  expect(await activePage(page)).toBe(base);
  expect(await scroll()).toBe(start);
  // Moving 300 ms into its slide, then still short of the page at 600 ms (the ease lands softly), there by 700.
  await page.clock.runFor(200 + 300);
  const mid = await scroll();
  expect(mid).toBeGreaterThan(start);
  expect(mid).toBeLessThan(start + width);
  // The incoming page's art lags its words: shifted against the slide by 15 percent of its offset.
  const lag = await page.evaluate((i) => {
    const p = document.querySelectorAll<HTMLElement>(".hero-page")[i]!;
    const offset = p.offsetLeft - document.querySelector(".hero-track")!.scrollLeft;
    const m = /translateX\((-?[\d.]+)px\)/.exec(p.querySelector<HTMLElement>(".hero-bd")!.style.transform);
    return { offset, shift: m ? Number(m[1]) : null };
  }, base + 1);
  expect(lag.shift).not.toBeNull();
  expect(lag.shift!).toBeCloseTo(-lag.offset * 0.15, 0);
  await page.clock.runFor(400);
  expect(Math.round(await scroll())).toBe(Math.round(start + width));
  await context.close();
});

for (const form of ["phone", "desktop"] as const) {
  test(`${form}: the owner's C2, a Top 10 place over the cover's top left, its genre under the art`, async ({ browser }) => {
    const context = await browser.newContext({ viewport: form === "phone" ? { width: 390, height: 844 } : { width: 1440, height: 900 }, reducedMotion: "reduce" });
    const page = await context.newPage();
    await page.goto(`/?form=${form}`);
    const row = page.locator(".row", { has: page.locator("h2", { hasText: "Top 10 Movies" }) });
    await row.scrollIntoViewIfNeeded({ timeout: 30_000 });
    const tile = row.locator(".tile").first();
    await expect(tile.locator(".rank")).toHaveText("1", { timeout: 30_000 });
    const box = await tile.evaluate((t) => {
      const pic = t.querySelector(".pic")!.getBoundingClientRect();
      const rank = t.querySelector(".rank")!.getBoundingClientRect();
      const genre = t.querySelector(".rank-genre")?.getBoundingClientRect();
      return { pic: [pic.left, pic.top, pic.width, pic.bottom], rank: [rank.left, rank.top, rank.right, rank.bottom], genreTop: genre?.top ?? null };
    });
    const [left = 0, top = 0, width = 0] = box.pic;
    // Inside the art, in its top left quarter.
    expect(box.rank[0] ?? -1).toBeGreaterThanOrEqual(left);
    expect(box.rank[1] ?? -1).toBeGreaterThanOrEqual(top);
    expect(box.rank[2] ?? Infinity).toBeLessThan(left + width / 2);
    // The genre under the art, never over the cover's ratings band.
    expect(box.genreTop).not.toBeNull();
    expect(box.genreTop!).toBeGreaterThanOrEqual(box.pic[3] ?? 0);
    await context.close();
  });
}

test("phone: the 8 s dwell never cuts a playing trailer", async ({ browser }) => {
  const { context, page } = await (await trailerPage(false))({ browser });
  const base = await activePage(page);
  await page.clock.runFor(1500);
  await expect.poll(() => page.evaluate(() => (window as unknown as { ytLog: string[] }).ytLog.some((l) => l.startsWith("playVideo"))), { timeout: 10_000 }).toBe(true);
  // Well past the dwell: the same page, the trailer still playing, its player not torn down.
  await page.clock.runFor(14_000);
  expect(await activePage(page)).toBe(base);
  await expect(page.locator(".hero.trailer-on")).toHaveCount(1);
  expect(await page.evaluate(() => (window as unknown as { ytLog: string[] }).ytLog.includes("destroy"))).toBe(false);
  await context.close();
});
