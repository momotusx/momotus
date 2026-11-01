import { type Browser, expect, test } from "./fixture";
import { GOT, PORTS, SHOTS } from "./core";

/** Layout rules the design audit settled (design/audit-web-2026-10-19.md). */

const FORMS = { tv: [1920, 1080, 1], desktop: [1440, 900, 1], phone: [390, 844, 2] } as const;
type Form = keyof typeof FORMS;

async function open(browser: Browser, form: Form, path: string) {
  const [width, height, scale] = FORMS[form];
  const context = await browser.newContext({
    viewport: { width, height },
    deviceScaleFactor: scale,
    isMobile: form === "phone",
    hasTouch: form === "phone",
    reducedMotion: "reduce",
  });
  const page = await context.newPage();
  return { context, page, go: () => page.goto(`${path}${path.includes("?") ? "&" : "?"}form=${form}`) };
}

for (const form of Object.keys(FORMS) as Form[]) {
  test(`${form}: the hero sits on the rows' grid line`, async ({ browser }) => {
    const { context, page, go } = await open(browser, form, "/");
    await go();
    await expect(page.locator(".hero-page.on .reason")).toBeVisible({ timeout: 30_000 });
    await expect(page.locator(".row-head").first()).toBeVisible({ timeout: 30_000 });
    const at = await page.evaluate((phone) => {
      const head = document.querySelector<HTMLElement>(".row-head") as HTMLElement;
      const r = head.getBoundingClientRect();
      const k = r.width / head.offsetWidth || 1;
      const grid = r.left + parseFloat(getComputedStyle(head).paddingLeft) * k;
      // TV and desktop: the words start on the line; phone: the hero is full bleed, its column centered.
      const hero = document.querySelector(phone ? ".hero" : ".hero-page.on .reason") as HTMLElement;
      return { grid: phone ? 0 : grid, left: hero.getBoundingClientRect().left };
    }, form === "phone");
    expect(Math.abs(at.left - at.grid)).toBeLessThanOrEqual(1);
    await context.close();
  });
}

for (const form of ["tv", "desktop"] as const) {
  test(`${form}: settings pages are one centered column, no empty right third`, async ({ browser }) => {
    const { context, page } = await open(browser, form, "/settings");
    for (const [path, block] of [
      ["/addons", ".addon"],
      ["/recommended-addons", "button.drow"],
      ["/profiles/manage", ".prow"],
    ] as const) {
      await page.goto(`${path}?form=${form}`);
      await expect(page.locator(block).first()).toBeVisible({ timeout: 30_000 });
      const box = await page.evaluate((sel) => {
        const r = (document.querySelector(sel) as HTMLElement).getBoundingClientRect();
        return { left: r.left, right: innerWidth - r.right, width: innerWidth };
      }, block);
      expect(Math.abs(box.left - box.right), `${path} is centered`).toBeLessThanOrEqual(2);
      expect(box.width - box.right, `${path} reaches past two thirds`).toBeGreaterThanOrEqual((box.width * 2) / 3);
    }
    await context.close();
  });
}

test("phone: a version row's quality starts on its size's line", async ({ browser }) => {
  const { context, page, go } = await open(browser, "phone", `/title/series/${GOT.id}/versions/${GOT.resume}`);
  await go();
  // The size leads the row's one line (plans/versions-web-spec.md 3.1 item 4).
  const row = page.locator(".vrow").filter({ hasText: /\d [KMG]B/ }).first();
  await expect(row).toBeVisible({ timeout: 30_000 });
  const tops = await row.evaluate((r) => ({
    q: (r.querySelector(".q") as HTMLElement).getBoundingClientRect().top,
    size: (r.querySelector(".st.line .lt") as HTMLElement).getBoundingClientRect().top,
  }));
  expect(Math.abs(tops.q - tops.size)).toBeLessThanOrEqual(3);
  await context.close();
});

// A phone wraps the link onto its own line under Save.
for (const form of ["tv", "desktop"] as const) {
  test(`${form}: the Debrid key page link is centered on Save`, async ({ browser }) => {
    const { context, page, go } = await open(browser, form, "/debrid");
    await go();
    const form_ = page.locator(".dkey").first();
    await expect(form_).toBeVisible({ timeout: 30_000 });
    const mid = await form_.evaluate((f) => {
      const c = (el: Element) => {
        const r = el.getBoundingClientRect();
        return r.top + r.height / 2;
      };
      return { link: c(f.querySelector(".dlink") as Element), save: c(f.querySelector('button[type="submit"]') as Element) };
    });
    expect(Math.abs(mid.link - mid.save)).toBeLessThanOrEqual(2);
    await context.close();
  });
}

test("tv: an episode whose still fails shows the series art with its number, not a dark box", async ({ browser }) => {
  const { context, page, go } = await open(browser, "tv", `/title/series/${GOT.id}?season=1`);
  // Every episode still fails to load.
  await context.route(/^https:\/\/episodes\.metahub\.space\//, (r) => r.fulfill({ status: 404, body: "" }));
  await go();
  await expect(page.locator(".ep-t").first()).toBeVisible({ timeout: 30_000 });
  const stills = page.locator(".ep-still");
  for (let i = 0; i < 4; i++) {
    const still = stills.nth(i);
    await expect(still.locator("img.stand-in.in")).toBeVisible({ timeout: 15_000 });
    await expect(still.locator(".ep-n")).toBeVisible();
  }
  await context.close();
});

for (const form of Object.keys(FORMS) as Form[]) {
  test(`${form}: recommended addons read at a glance: filled when on, a mark each, one white primary`, async ({ browser }) => {
    const { context, page, go } = await open(browser, form, "/welcome?step=2");
    await go();
    const rows = page.locator("button.drow");
    await expect(rows.first()).toBeVisible({ timeout: 30_000 });
    // A screen reader meets a switch with its name and its state.
    await expect(page.getByRole("switch", { name: /^Trakt/ })).toHaveAttribute("aria-checked", "true");
    // One switch turned off, so both states are on screen.
    if (form === "phone") await rows.first().tap();
    else await rows.first().click();
    await expect(rows.first()).toHaveAttribute("aria-checked", "false");
    const tracks = await rows.evaluateAll((els) =>
      els.map((r) => ({ on: r.getAttribute("aria-checked") === "true", track: getComputedStyle(r.querySelector(".switch") as Element).backgroundColor })),
    );
    expect(tracks.some((t) => t.on)).toBe(true);
    // On is the form's white primary track (the phone's #F5F5F5), off an empty one: the state reads without the word.
    const on = form === "phone" ? "rgb(245, 245, 245)" : "rgba(255, 255, 255, 0.92)";
    for (const t of tracks) expect(t.track).toBe(t.on ? on : "rgba(0, 0, 0, 0)");
    // Every row shows its mark: the logo, or the name's initial.
    const marks = await page.locator(".drow").evaluateAll((els) =>
      els.map((r) => ({ name: r.querySelector(".dtext b")?.textContent ?? "", mark: r.querySelector(".dlogo img") ? "logo" : (r.querySelector(".dlogo b")?.textContent ?? "") })),
    );
    expect(marks.length).toBeGreaterThan(0);
    for (const m of marks) expect(m.mark === "logo" || m.mark === m.name.charAt(0), `${m.name} has a mark`).toBe(true);
    // Continue is the one white primary.
    await expect(page.locator(".btn.primary")).toHaveCount(1);
    await expect(page.locator(".btn.primary")).toContainText("Continue");
    if (form !== "phone") {
      const gaps = await page.locator(".dgroups").evaluate((g) => {
        const r = g.getBoundingClientRect();
        return { left: r.left, right: innerWidth - r.right };
      });
      expect(Math.abs(gaps.left - gaps.right), "the step is centered").toBeLessThanOrEqual(2);
    }
    await context.close();
  });
}

// Fifth pass (design/audit-web-2026-10-19.md): what still read below the bar
// of the Apple TV app, Netflix and Infuse on the viewer's core flow.

const SHOT = SHOTS;
/** A logo of a given shape, served for every title's logo request. */
const logoSvg = (w: number, h: number) =>
  `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}"><rect width="${w}" height="${h}" rx="${Math.min(w, h) / 5}" fill="#e9e2d2"/></svg>`;

/** The drawn logo's size in canvas px (the image fits its box, contained). */
async function drawnLogo(browser: Browser, form: Form, path: string, selector: string, w: number, h: number, shot?: string) {
  const { context, page, go } = await open(browser, form, path);
  await context.route(/^https:\/\/images\.metahub\.space\/logo\//, (r) =>
    r.fulfill({ status: 200, contentType: "image/svg+xml", headers: { "access-control-allow-origin": "*" }, body: logoSvg(w, h) }),
  );
  // A phone's title page shows a metahub cover with no logo (it prints the title): with no cover the logo is back.
  if (form === "phone" && path !== "/") await context.route(/^https:\/\/images\.metahub\.space\/poster\//, (r) => r.fulfill({ status: 404 }));
  await go();
  const logo = page.locator(selector).first();
  await expect(logo).toBeVisible({ timeout: 30_000 });
  await expect(logo).not.toHaveClass(/pending/);
  await page.waitForTimeout(300);
  if (shot) await page.screenshot({ path: `${SHOT}/${shot}-${form}.jpg`, type: "jpeg", quality: 80 });
  // On Home the top of the hero's stack (its logo) stays clear of the navigation whatever the
  // logo's shape, over a synopsis that runs long (the layout rhythm's rule).
  const clear =
    path === "/" && form !== "phone"
      ? await page.evaluate(() => {
          const syn = document.querySelector(".hero-page.on .hero-syn");
          if (syn) syn.textContent = "A long synopsis. ".repeat(40);
          const top = document.querySelector(".hero-page.on .hero-logo:not(.sr)")?.getBoundingClientRect().top ?? 0;
          const nav = document.querySelector(".topnav .tabbar")?.getBoundingClientRect().bottom ?? 0;
          return top - nav;
        })
      : null;
  if (clear !== null) expect(clear, `${w}x${h} logo: the stack below the navigation`).toBeGreaterThan(12);
  const box = await logo.evaluate((el) => {
    const img = el as HTMLImageElement;
    const r = img.naturalWidth / img.naturalHeight;
    const bw = img.offsetWidth;
    const bh = img.offsetHeight;
    const dw = Math.min(bw, bh * r);
    return { w: dw, h: dw / r };
  });
  await context.close();
  return box;
}

for (const form of ["tv", "desktop", "phone"] as const) {
  test(`${form}: a tall logo and a wide one carry about the same weight, on Home's hero and on Title`, async ({ browser }) => {
    for (const [path, selector] of [
      ["/", ".hero-logo"],
      [`/title/series/${GOT.id}`, ".title-logo"],
    ] as const) {
      const name = path === "/" ? "audit5-logo-home" : "audit5-logo-title";
      const tall = await drawnLogo(browser, form, path, selector, 300, 360, `${name}-tall`);
      const wide = await drawnLogo(browser, form, path, selector, 1200, 150, `${name}-wide`);
      const ratio = (wide.w * wide.h) / (tall.w * tall.h);
      // A stacked logo is no stamp beside a wide one (Netflix sizes logos by area).
      // Within 2.5 times (one height for all drew 4.7 times); the hero caps
      // a stacked logo at 140 px so its stack still clears the pill.
      expect(ratio, `${path}: wide ${Math.round(wide.w)}x${Math.round(wide.h)}, tall ${Math.round(tall.w)}x${Math.round(tall.h)}`).toBeLessThan(2.5);
      expect(ratio).toBeGreaterThan(1 / 2.5);
    }
  });
}

test("tv: paused, the picture dims and the title says what it is", async ({ browser }) => {
  const { context, page, go } = await open(browser, "tv", `/play/movie/tt1254207/tt1254207`);
  await go();
  const toggle = page.locator('[data-f="player:toggle"]');
  await expect(toggle).toBeVisible({ timeout: 30_000 });
  // A remote has Back of its own: no on-screen Back, full screen or volume.
  await expect(page.locator('[data-f="player:leave"], [data-f="player:fullscreen"], [data-f="player:mute"]')).toHaveCount(0);
  await page.waitForFunction(() => (document.querySelector("video")?.readyState ?? 0) >= 2);
  if ((await toggle.getAttribute("aria-label")) === "Play") await toggle.press("Enter");
  await expect(toggle).toHaveAttribute("aria-label", "Pause");
  // The first frame played (the dim is for a pause after it, not before).
  await expect(page.locator(".player-scr")).toHaveClass(/\bstarted\b/, { timeout: 30_000 });
  // Playing: no dim, no synopsis.
  await expect(page.locator(".player-scr")).not.toHaveClass(/\bpaused\b/);
  await toggle.press("Enter");
  await expect(toggle).toHaveAttribute("aria-label", "Play");
  await page.waitForTimeout(500);
  await page.screenshot({ path: `${SHOT}/audit5-paused-tv.jpg`, type: "jpeg", quality: 80 });
  await expect(page.locator(".player-scr")).toHaveClass(/\bpaused\b/);
  await expect(page.locator(".psyn")).toBeVisible();
  await expect(page.locator(".psyn")).toContainText(/\w+/);
  expect(await page.locator(".pdim").evaluate((d) => getComputedStyle(d).opacity)).toBe("1");
  // Playing again clears both.
  await toggle.press("Enter");
  await expect(page.locator(".player-scr")).not.toHaveClass(/\bpaused\b/);
  await expect(page.locator(".psyn")).toBeHidden();
  await context.close();
});

test("tv: a still that is still loading pulses instead of sitting as a dark box", async ({ browser }) => {
  const { context, page, go } = await open(browser, "tv", "/");
  // Every image waits until released, as on a slow link.
  let release: () => void = () => {};
  const held = new Promise<void>((ok) => (release = ok));
  await context.route(/^https:\/\/(images|episodes)\.metahub\.space\//, async (r) => {
    await held;
    await r.continue();
  });
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await go();
  const pic = page.locator('[data-f^="tile:home:continue-watching:"] .pic').nth(1);
  await expect(pic).toBeVisible({ timeout: 30_000 });
  await page.screenshot({ path: `${SHOT}/audit5-loading-tv.jpg`, type: "jpeg", quality: 80 });
  expect(await pic.evaluate((p) => getComputedStyle(p).animationName)).toBe("arriving");
  // The surface breathes, not the box: the focus ring on it stays solid.
  expect(await pic.evaluate((p) => getComputedStyle(p).opacity)).toBe("1");
  release();
  await expect(pic.locator("img.in")).toBeVisible({ timeout: 30_000 });
  expect(await pic.evaluate((p) => getComputedStyle(p).animationName)).toBe("none");
  await context.close();
});

test("phone: an episode's title is read whole, on up to two lines", async ({ browser }) => {
  const { context, page, go } = await open(browser, "phone", `/title/series/${GOT.id}?season=1`);
  await go();
  const titles = page.locator(".ep-t");
  // Season 1 has "Cripples, Bastards, and Broken Things".
  await expect(titles.first()).toHaveText("Winter Is Coming", { timeout: 30_000 });
  await page.waitForTimeout(300);
  await page.locator(".ep").nth(2).scrollIntoViewIfNeeded();
  await page.screenshot({ path: `${SHOT}/audit5-episodes-phone.jpg`, type: "jpeg", quality: 80 });
  const cut = await titles.evaluateAll((els) =>
    els.slice(0, 10).filter((t) => t.scrollWidth > t.clientWidth + 1 || t.scrollHeight > t.clientHeight + 1).map((t) => t.textContent),
  );
  expect(cut).toEqual([]);
  await context.close();
});

test("phone: a meta line that wraps never ends or starts a line with a dot", async ({ browser }) => {
  const { context, page, go } = await open(browser, "phone", `/title/series/${GOT.id}`);
  await go();
  const line = page.locator(".info .meta");
  await expect(line).toContainText("seasons", { timeout: 30_000 });
  await page.waitForTimeout(400);
  const at = await line.evaluate((el) => {
    const mid = (e: Element) => {
      const r = e.getBoundingClientRect();
      return (r.top + r.bottom) / 2;
    };
    const kids = [...el.children];
    const lines = new Set(kids.map((k) => Math.round(mid(k) / 10))).size;
    const stray = [...el.querySelectorAll(":scope > b")]
      .filter((b) => getComputedStyle(b).visibility !== "hidden")
      .filter((b) => {
        const p = b.previousElementSibling;
        const n = b.nextElementSibling;
        return !p || !n || Math.abs(mid(p) - mid(b)) > 8 || Math.abs(mid(n) - mid(b)) > 8;
      })
      .map((b) => `${b.previousElementSibling?.textContent} | ${b.nextElementSibling?.textContent}`);
    return { lines, stray };
  });
  // It wraps on a phone (else this proves nothing), and no dot hangs at a break.
  expect(at.lines).toBeGreaterThan(1);
  expect(at.stray).toEqual([]);
  await context.close();
});

test("desktop and phone players say Back on screen; a desktop adds volume and full screen", async ({ browser }) => {
  for (const form of ["desktop", "phone"] as const) {
    const { context, page, go } = await open(browser, form, `/play/movie/tt1254207/tt1254207`);
    // Full screen asked for is counted; a headless page cannot enter it.
    await page.addInitScript(() => {
      const w = window as unknown as { fullAsked: number };
      w.fullAsked = 0;
      Element.prototype.requestFullscreen = () => {
        w.fullAsked++;
        return Promise.resolve();
      };
    });
    await go();
    const back = page.locator('[data-f="player:leave"]');
    await expect(back).toBeVisible({ timeout: 30_000 });
    await expect(back).toHaveAttribute("aria-label", "Back");
    await page.waitForFunction(() => (document.querySelector("video")?.readyState ?? 0) >= 1, null, { timeout: 30_000 });
    await page.evaluate(() => {
      const v = document.querySelector("video");
      if (v) v.muted = true;
    });
    const volume = () => page.evaluate(() => document.querySelector("video")?.volume ?? -1);
    if (form === "desktop") {
      const mute = page.locator('[data-f="player:mute"]');
      await mute.dispatchEvent("click");
      await expect.poll(volume).toBe(0);
      await expect(mute).toHaveAttribute("aria-label", "Unmute");
      await mute.dispatchEvent("click");
      await expect.poll(volume).toBe(1);
      await page.locator('[data-f="player:volume"]').evaluate((el) => {
        (el as HTMLInputElement).value = "30";
        el.dispatchEvent(new Event("input", { bubbles: true }));
      });
      await expect.poll(volume).toBeCloseTo(0.3, 2);
      await page.locator('[data-f="player:fullscreen"]').dispatchEvent("click");
      await expect.poll(() => page.evaluate(() => (window as unknown as { fullAsked: number }).fullAsked)).toBe(1);
    } else {
      await expect(page.locator('[data-f="player:fullscreen"], [data-f="player:mute"], [data-f="player:volume"]')).toHaveCount(0);
    }
    await page.mouse.move(400, 300);
    await page.screenshot({ path: `${SHOT}/player-controls-${form}.jpg`, type: "jpeg", quality: 80 });
    // Back with no history in this tab: the title, the player's parent.
    await back.dispatchEvent("click");
    await expect(page).toHaveURL(/\/title\/movie\/tt1254207/);
    await context.close();
  }
});

test("tv: with no backdrop from any catalog, Home's hero closes once the rows have answered", async ({ browser }) => {
  const [width, height, scale] = FORMS.tv;
  const context = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: scale, reducedMotion: "reduce" });
  // Catalogs that give no backdrop (posters only, or offline): the core's
  // Home has no hero. Made up at the socket over the real Home.
  await context.routeWebSocket(/\/ws$/, (ws) => {
    const upstream = new WebSocket(`ws://127.0.0.1:${PORTS.bridge}/ws`);
    const early: string[] = [];
    const homes = new Set<number>();
    const asked = new Set<number>();
    upstream.addEventListener("open", () => {
      for (const m of early.splice(0)) upstream.send(m);
    });
    upstream.addEventListener("message", (e) => {
      const m = JSON.parse(String(e.data));
      if (m.id !== undefined && asked.has(m.id) && m.ok) {
        homes.add(m.result.sub);
        m.result.snapshot.header.heroes = [];
      } else if (m.sub !== undefined && homes.has(m.sub) && Array.isArray(m.updates)) {
        for (const u of m.updates) if (u.kind === "header") u.header.heroes = [];
      }
      ws.send(JSON.stringify(m));
    });
    upstream.addEventListener("close", () => ws.close());
    ws.onMessage((raw) => {
      const m = JSON.parse(String(raw));
      if (m.op === "subscribe" && m.view === "home") asked.add(m.id);
      if (upstream.readyState === WebSocket.OPEN) upstream.send(String(raw));
      else early.push(String(raw));
    });
    ws.onClose(() => upstream.close());
  });
  const page = await context.newPage();
  await page.goto("/?form=tv");
  // While a row is on its way the hero may still come; once every row has
  // answered with no backdrop, the skeleton goes and the hero closes.
  await expect(page.locator(".row").first()).toBeVisible({ timeout: 30_000 });
  await expect(page.locator(".hero-skel")).toHaveCount(0, { timeout: 60_000 });
  // Closed, the hero keeps only the room under the tab bar: the first row's title starts under it.
  await expect(page.locator(".hero.closed .hero-bd")).toBeHidden();
  const head = await page.locator(".row .row-head h2").first().evaluate((el) => Math.round(el.getBoundingClientRect().top));
  const nav = await page.locator(".topnav .tabbar").evaluate((el) => Math.round(el.getBoundingClientRect().bottom));
  expect(head).toBeGreaterThan(nav + 12);
  expect(head).toBeLessThan(nav + 80);
  await context.close();
});

test("desktop: Discover keeps the viewer's type and place on Back", async ({ browser }) => {
  const { context, page, go } = await open(browser, "desktop", "/discover");
  await go();
  // Another type than the first one Discover opens on.
  const kinds = page.locator('[data-fgroup="kinds"] [data-f]');
  await expect(kinds.nth(1)).toBeVisible({ timeout: 30_000 });
  const kind = (await kinds.nth(1).getAttribute("data-f")) as string;
  await kinds.nth(1).click();
  await expect(page.locator(`[data-f="${kind}"]`)).toHaveClass(/\bon\b/);
  const tiles = page.locator('[data-fgroup^="grid:"] [data-f]');
  await expect(tiles.nth(12)).toBeVisible({ timeout: 30_000 });
  // Down the page, then a title, then Back.
  const page_ = page.locator("main.page");
  await page_.evaluate((el) => el.scrollTo({ top: 600 }));
  await expect.poll(() => page_.evaluate((el) => el.scrollTop)).toBeGreaterThan(500);
  await tiles.nth(12).click();
  await expect(page).toHaveURL(/\/title\//);
  await page.goBack();
  await expect(page).toHaveURL(/\/discover/);
  await expect(page.locator(`[data-f="${kind}"]`)).toHaveClass(/\bon\b/, { timeout: 10_000 });
  await expect.poll(() => page.locator("main.page").evaluate((el) => el.scrollTop)).toBeGreaterThan(500);
  await context.close();
});

test("desktop: Library keeps the viewer's filter and sort on Back", async ({ browser }) => {
  const { context, page, go } = await open(browser, "desktop", "/library");
  await go();
  await expect(page.locator('[data-fgroup="lib-sort"] [data-f]').first()).toBeVisible({ timeout: 30_000 });
  const name = page.locator('[data-f="lib-sort:name"]');
  await name.click();
  await expect(name).toHaveClass(/\bon\b/);
  await page.locator('[data-fgroup^="grid:"] [data-f]').first().click();
  await expect(page).toHaveURL(/\/title\//);
  await page.goBack();
  await expect(page).toHaveURL(/\/library/);
  await expect(page.locator('[data-f="lib-sort:name"]')).toHaveClass(/\bon\b/, { timeout: 10_000 });
  await context.close();
});

test("desktop: Setup check on a page that does not exist opens the setup check, not the first-run doors", async ({ browser }) => {
  const { context, page, go } = await open(browser, "desktop", "/no-such-page");
  await go();
  const setup = page.locator('[data-f="stub:setup"]');
  await expect(setup).toBeVisible({ timeout: 30_000 });
  await expect(setup).toHaveAttribute("href", "/welcome?step=3");
  await setup.click();
  await expect(page).toHaveURL(/\/welcome\?step=3/);
  await expect(page.locator('[data-f="welcome:stremio"]')).toHaveCount(0);
  await context.close();
});

test("phone: a tab root ends clear of the floating tab bar", async ({ browser }) => {
  const { context, page, go } = await open(browser, "phone", "/library");
  await go();
  await expect(page.locator(".grid .tile").first()).toBeVisible({ timeout: 30_000 });
  await page.evaluate(() => window.scrollTo(0, 1e6));
  const [last, bar] = await page.evaluate(() => [
    [...document.querySelectorAll(".grid .tile")].at(-1)?.getBoundingClientRect().bottom ?? 0,
    document.querySelector(".tabs")?.getBoundingClientRect().top ?? 0,
  ]);
  expect(last).toBeLessThanOrEqual((bar as number) - 8);
  await context.close();
});
