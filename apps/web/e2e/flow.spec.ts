import { expect, type Page, test } from "@playwright/test";
import { Client, GOT, reseedBbb, startIfPaused, watch } from "./core";
import { BBB_ID, SAMPLE_EPISODE } from "./source";

test.use({ viewport: { width: 1920, height: 1080 } });

const focused = (page: Page) => page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f ?? null);
const cw = (id: string) => `tile:home:continue-watching:${id}`;

test("remote flow: Home, Title, seasons, Versions and Back restore focus", async ({ page }) => {
  await page.goto("/?form=tv");

  // Continue Watching is the first row and takes the initial focus.
  await expect(page.locator(`[data-f="${cw(BBB_ID)}"]`)).toBeVisible();
  // The hero's buttons name the title they act on.
  await expect(page.locator('[data-f="hero:details"]')).toHaveAttribute("aria-label", /^Details for \S/);
  await expect.poll(() => focused(page)).toBe(cw(BBB_ID));
  await expect(page.locator(".row").nth(1).locator(".tile").first()).toBeVisible();

  // Down into the next row, Up back (the row remembers its tile), Right, Enter.
  await page.keyboard.press("ArrowDown");
  // Row ids are opaque; the installation tag after the addon id varies.
  await expect.poll(() => focused(page)).toMatch(/^tile:home:com\.linvo\.cinemeta[@:]/);
  await page.keyboard.press("ArrowUp");
  await expect.poll(() => focused(page)).toBe(cw(BBB_ID));
  await page.keyboard.press("ArrowRight");
  await expect.poll(() => focused(page)).toBe(cw(GOT.id));
  await page.keyboard.press("Enter");

  // Real Game of Thrones data from Cinemeta, resuming where the seed left off.
  await expect(page).toHaveURL(/\/title\/series\/tt0944947/);
  // The logo is the picture; the page's heading still says the name.
  await expect(page.locator("img.title-logo")).toBeVisible();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Game of Thrones");
  // The ratings lead the meta line as plain text, IMDb first.
  await expect(page.locator(".info .meta .rate").first()).toHaveText(/^IMDb \d/);
  await expect(page.locator('[data-f="title:play"]')).toContainText("Resume S2:E3");
  await expect(page.locator('[data-f="season:2"]')).toHaveAttribute("aria-selected", "true");
  await expect(page.locator(".ep-t").first()).toHaveText("The North Remembers");

  // Season switching keeps focus on the tab.
  await page.locator('[data-f="season:3"]').focus();
  await page.keyboard.press("Enter");
  await expect(page.locator('[data-f="season:3"]')).toHaveAttribute("aria-selected", "true");
  await expect(page.locator(".ep-t").first()).toHaveText("Valar Dohaeris");
  await expect.poll(() => focused(page)).toBe("season:3");
  await page.locator('[data-f="season:2"]').focus();
  await page.keyboard.press("Enter");
  await expect(page.locator(".ep-t").first()).toHaveText("The North Remembers");

  // Versions: the live view from the installed stream addons.
  await page.locator('[data-f="title:versions"]').focus();
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/\/versions\/tt0944947:2:3/);
  await expect(page.locator(".vhead h1")).toContainText("S2:E3 What Is Dead May Never Die");
  // The breadcrumb starts at the title, wherever the viewer came from.
  await expect(page.locator(".crumbs > span")).toHaveText(["Game of Thrones", "Versions"]);
  await expect(page.locator(".pick h2")).toBeVisible();
  await expect(page.locator(".vrow").first()).toBeVisible();
  await expect.poll(() => focused(page)).toBe("versions:play");

  // Back twice lands on Home with focus on the tile that opened the title.
  await page.keyboard.press("Escape");
  await expect(page).toHaveURL(/\/title\/series\/tt0944947/);
  await page.keyboard.press("Escape");
  await expect(page).toHaveURL(/\/\?form=tv$|\/$/);
  await expect.poll(() => focused(page)).toBe(cw(GOT.id));
});

test("plays a direct source through the core's stream server", async ({ page }) => {
  await page.goto(`/play/movie/${BBB_ID}/${BBB_ID}?form=tv`);
  await expect(page.locator(".ptitle h1")).toHaveText("Big Buck Bunny");
  await expect(page.locator('[data-f="player:toggle"]')).toBeVisible({ timeout: 30_000 });
  await expect.poll(() => focused(page)).toBe("player:toggle");
  await startIfPaused(page);
  // Resumes from saved progress (the seed stopped at 2:30) and keeps playing.
  const time = () => page.evaluate(() => document.querySelector("video")?.currentTime ?? 0);
  await expect.poll(time, { timeout: 30_000 }).toBeGreaterThanOrEqual(150);
  await expect.poll(time, { timeout: 30_000 }).toBeGreaterThan(153);
  const src = await page.evaluate(() => document.querySelector("video")?.src ?? "");
  expect(src).toMatch(/^http:\/\/127\.0\.0\.1:\d+\//);
  // Actions in one order everywhere: Audio, Subtitles, Versions, Speed.
  expect(await page.locator(".pacts .pa b").allTextContents()).toEqual(["Audio", "Subtitles", "Versions", "Speed"]);

  // Subtitles from OpenSubtitles load into the video as a text track, all by
  // remote: the first press only wakes the hidden controls (Left and Right
  // skip 10 s then, journey-api.spec).
  await page.waitForTimeout(4500);
  await expect(page.locator(".player-scr")).toHaveClass(/idle/);
  await page.keyboard.press("ArrowUp");
  await expect(page.locator(".player-scr")).not.toHaveClass(/idle/);
  await expect.poll(() => focused(page)).toBe("player:toggle");
  for (let i = 0; i < 4 && (await focused(page)) !== "player:subs"; i++) await page.keyboard.press("ArrowRight");
  await expect.poll(() => focused(page)).toBe("player:subs");
  await page.keyboard.press("Enter");
  await expect.poll(() => focused(page)).toBe("sub:off");
  // Options go by their file's address, so a late addon never renames them.
  const first = page.locator(".sub-lang .opt").first();
  await expect(first).toBeVisible({ timeout: 30_000 });
  await page.keyboard.press("ArrowDown");
  await expect.poll(() => focused(page)).toBe(await first.getAttribute("data-f"));
  expect(await first.getAttribute("data-f")).toMatch(/^sub:http/);
  await page.keyboard.press("Enter");
  // Big Buck Bunny has almost no dialogue, so any cue proves the track loaded.
  await expect
    .poll(() => page.evaluate(() => document.querySelector("video")?.textTracks[0]?.cues?.length ?? 0), { timeout: 30_000 })
    .toBeGreaterThan(0);
  expect(await page.evaluate(() => document.querySelector("video")?.textTracks[0]?.mode)).toBe("showing");
  await expect.poll(() => focused(page)).toBe("player:subs");
  await expect(page.locator('[data-f="player:subs"] small')).not.toContainText("Off");

  // Back goes to the title, never an exit dialog.
  await page.keyboard.press("Escape");
  await expect(page).toHaveURL(new RegExp(`/title/movie/${BBB_ID}`));
});

test("Skip credits from skip data in TheIntroDB's shape, by remote, only on a press", async ({ page }) => {
  await page.goto(`/play/movie/${BBB_ID}/${BBB_ID}?form=tv`);
  await expect(page.locator('[data-f="player:toggle"]')).toBeVisible({ timeout: 30_000 });
  await startIfPaused(page);
  const time = () => page.evaluate(() => document.querySelector("video")?.currentTime ?? 0);
  const seekTo = (s: number) =>
    page.evaluate((at) => {
      const v = document.querySelector("video");
      if (v) v.currentTime = at;
    }, s);
  await expect.poll(time, { timeout: 30_000 }).toBeGreaterThanOrEqual(150);
  try {
    await skipCredits(page, time, seekTo);
  } finally {
    // Back to the seeded position for the tests that follow, pass or fail.
    await page.evaluate(() => {
      const v = document.querySelector("video");
      if (v) {
        v.currentTime = 150;
        v.pause();
      }
    });
    await expect.poll(time).toBeLessThan(151);
    await page.waitForTimeout(500);
  }
});

async function skipCredits(page: Page, time: () => Promise<number>, seekTo: (s: number) => Promise<void>) {
  const skip = page.locator('[data-f="player:skip"]');
  await expect(skip).toHaveCount(0);

  // The skip fixture (synthetic, in TheIntroDB's shape) marks the credits
  // from 8:00 to 9:50; the core looks them up once the player reports the
  // file's length.
  await seekTo(493);
  await expect(skip).toHaveText("Skip credits", { timeout: 30_000 });
  await expect.poll(() => focused(page)).toBe("player:skip");
  // A white pill with no edge: on TV the ring is white (the TV form has no key color).
  const look = () =>
    skip.evaluate((el) => {
      const s = getComputedStyle(el);
      return { fill: s.backgroundColor, edge: s.boxShadow.includes("inset"), ring: s.outlineStyle === "none" || s.outlineColor === "rgba(0, 0, 0, 0)" ? null : s.outlineColor };
    });
  await expect.poll(look).toEqual({ fill: "rgba(255, 255, 255, 0.92)", edge: false, ring: "rgb(255, 255, 255)" });
  await expect(page.locator(".skip-src")).toHaveText("Skip data: TheIntroDB");
  await expect(page.locator(".skip-src")).toHaveCSS("opacity", "1");

  // It never jumps by itself: the credits keep playing until the press, and
  // the button stays on screen with the controls hidden.
  await expect(page.locator(".player-scr")).toHaveClass(/idle/, { timeout: 10_000 });
  await expect(skip).toBeVisible();
  const t = await time();
  expect(t).toBeGreaterThan(494);
  expect(t).toBeLessThan(510);
  // Hidden controls: bottom right, inside the band of the control row (its
  // height follows the action labels, so the band, not one centre line).
  const box = (sel: string) =>
    page.evaluate((s) => {
      const r = document.querySelector(s)?.getBoundingClientRect();
      return r ? { top: r.top, bottom: r.bottom, mid: (r.top + r.bottom) / 2 } : { top: 0, bottom: 0, mid: 0 };
    }, sel);
  const inRow = async () => {
    const [b, row] = [await box('[data-f="player:skip"]'), await box(".ctl")];
    return b.mid >= row.top && b.mid <= row.bottom && b.bottom > 1080 * 0.88;
  };
  await expect.poll(inRow).toBe(true);

  // One press acts at once, without first waking the controls.
  await page.keyboard.press("Enter");
  await expect.poll(time).toBeGreaterThanOrEqual(589.9);
  await expect(skip).toHaveCount(0);
  await expect.poll(() => focused(page)).toBe("player:toggle");

  // With the controls up (the press woke them) it sits above the progress bar.
  await seekTo(493);
  await expect(skip).toBeVisible();
  await expect(page.locator(".player-scr")).not.toHaveClass(/idle/);
  await expect.poll(async () => (await box(".scrub")).top - (await box('[data-f="player:skip"]')).bottom).toBeGreaterThan(16);
  // Without the focus, no ring and still no edge.
  await skip.evaluate((el) => (el as HTMLElement).blur());
  await expect.poll(look).toEqual({ fill: "rgba(255, 255, 255, 0.92)", edge: false, ring: null });
}

test("movie page: trailer overlay and Mark watched by remote", async ({ page }) => {
  await page.goto(`/title/movie/${BBB_ID}?form=tv`);
  await expect.poll(() => focused(page)).toBe("title:play");

  // The trailer comes from the meta's trailer list and plays over the page.
  await page.locator('[data-f="title:trailer"]').focus();
  await page.keyboard.press("Enter");
  const frame = page.locator(".trailer iframe");
  await expect(frame).toHaveAttribute("src", /^https:\/\/www\.youtube-nocookie\.com\/embed\/yUQM7H4Swgw\?autoplay=1/);
  await expect.poll(() => focused(page)).toBe("trailer:close");
  // The page behind is out of reach while the trailer is open.
  await expect(page.locator(".title-main")).toHaveAttribute("inert", /.*/);
  await page.keyboard.press("ArrowUp");
  await expect.poll(() => focused(page)).toBe("trailer:close");
  // Left reaches Pause on the bar, and nothing above it.
  await page.keyboard.press("ArrowLeft");
  await expect.poll(() => focused(page)).toBe("trailer:toggle");
  await page.keyboard.press("ArrowUp");
  await expect.poll(() => focused(page)).toBe("trailer:toggle");
  // Back closes it and focus returns to the button.
  await page.keyboard.press("Escape");
  await expect(page.locator(".trailer")).toHaveCount(0);
  await expect(page).toHaveURL(new RegExp(`/title/movie/${BBB_ID}`));
  await expect.poll(() => focused(page)).toBe("title:trailer");

  // Mark watched toggles the movie's own flag in the core, both ways, and
  // Play says what it will do: the core's play target label, shown as is.
  const watched = page.locator('[data-f="title:watched"]');
  const play = page.locator('[data-f="title:play"]');
  await expect(watched).toHaveAttribute("aria-pressed", "false");
  await expect(play).toContainText("Resume");
  await expect(play).toContainText("min left");
  await watched.focus();
  await page.keyboard.press("Enter");
  await expect(watched).toHaveAttribute("aria-pressed", "true");
  await expect(watched).toHaveText("Watched");
  // Watched: start over, with no resume point beside it.
  await expect(play).toHaveText("Play again");
  await page.keyboard.press("Enter");
  await expect(watched).toHaveAttribute("aria-pressed", "false");
  await expect(watched).toHaveText("Mark as watched");
  await expect(play).toContainText("Resume");
});

test("onboarding: one decision per step, Back walks the steps", async ({ page }) => {
  await page.goto("/welcome?form=tv");
  await expect.poll(() => focused(page)).toBe("welcome:stremio");
  await expect(page.locator(".door")).toHaveCount(3);
  await expect(page.locator(".check")).toHaveCount(0);
  await expect(page.locator('[data-f="welcome:skip"]')).toHaveText("Browse first");
  await page.keyboard.press("ArrowRight");
  await expect.poll(() => focused(page)).toBe("welcome:fresh");
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/step=2/);
  await expect(page.locator("h1")).toHaveText("Recommended addons");
  // No typing first on a remote: focus lands on Continue, and there is no link field.
  await expect.poll(() => focused(page)).toBe("welcome:next");
  await expect(page.locator('[data-f="welcome:url"]')).toHaveCount(0);
  // A default this profile has shows its state in words, without a switch.
  await expect(page.locator(".drow.fixed", { hasText: "Cinemeta" })).toContainText("Installed");
  await expect(page.locator('[data-f="welcome:default:trakt"]')).toHaveAttribute("aria-checked", "true");
  // Continue would add the defaults to the shared core: Back instead.
  await page.keyboard.press("Escape");
  await expect(page).toHaveURL(/\/welcome(\?form=tv)?$/);
  await page.goto("/welcome?step=3&form=tv");
  await expect(page.locator(".check li").first()).toBeVisible();
  await expect(page.locator(".check")).not.toContainText("Core");
});

test("no dead ends: Add a source when nothing plays, a card when nothing plays in the player", async ({ page }) => {
  // Tom & Cherry: no addon here lists a version this browser can play.
  await page.goto("/title/movie/tt43718175?form=tv");
  const add = page.locator('[data-f="title:addsource"]');
  await expect(add).toHaveText("Add a source", { timeout: 60_000 });
  await expect(page.locator('[data-f="title:play"]')).toHaveCount(0);
  await expect.poll(() => focused(page)).toBe("title:addsource");
  // One wording for it on the title page and in the player.
  const none = "No playable version found";
  const why = "None of your addons found a stream for this. A source addon finds them; add one in Addons.";
  await expect(page.locator(".vline")).toContainText(`${none}. ${why}`);

  // Straight into the player: a centered card offers a version or Back.
  await page.goto("/play/movie/tt43718175/tt43718175?form=tv");
  await expect(page.locator(".endcard .st")).toHaveText(none, { timeout: 60_000 });
  await expect(page.locator(".endcard .end-detail")).toHaveText(why);
  await expect.poll(() => focused(page)).toBe("player:versions");
  await expect(page.locator('[data-f="player:versions"]')).toHaveText("Choose a version");
  await page.keyboard.press("ArrowRight");
  await expect.poll(() => focused(page)).toBe("player:back");
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/\/title\/movie\/tt43718175/);
});

test("a movie ends on a card: Back, or Play again", async ({ page }) => {
  await page.goto(`/play/movie/${BBB_ID}/${BBB_ID}?form=tv`);
  await expect(page.locator('[data-f="player:toggle"]')).toBeVisible({ timeout: 30_000 });
  await startIfPaused(page);
  const time = () => page.evaluate(() => document.querySelector("video")?.currentTime ?? 0);
  await expect.poll(time, { timeout: 30_000 }).toBeGreaterThan(150);
  try {
    await page.evaluate(() => {
      const v = document.querySelector("video");
      if (v) v.currentTime = v.duration - 2;
    });
    const card = page.locator(".endcard");
    await expect(card).toBeVisible({ timeout: 15_000 });
    await expect(card.locator("h2")).toHaveText("Big Buck Bunny");
    await expect.poll(() => focused(page)).toBe("end:back");
    await expect(page.locator('[data-f="end:back"]')).toHaveText("Back");
    await expect(page.locator(".osd")).toHaveCount(0);
    await page.keyboard.press("ArrowRight");
    await expect.poll(() => focused(page)).toBe("end:again");
    await page.keyboard.press("Enter");
    await expect(card).toHaveCount(0);
    await expect.poll(time).toBeLessThan(5);
  } finally {
    await page.evaluate(() => document.querySelector("video")?.pause());
    await reseedBbb(page);
  }
});

test("an episode row plays that episode, not the resume target", async ({ page }) => {
  await page.goto(`/title/series/${GOT.id}?form=tv`);
  // The resume target is S2:E3 (seeded); pick S1:E5 instead, by remote.
  await expect.poll(() => focused(page)).toBe("title:play");
  await page.locator('[data-f="season:1"]').focus();
  await page.keyboard.press("Enter");
  const row = page.locator(`[data-f="ep:${SAMPLE_EPISODE}"]`);
  await expect(row).toBeVisible({ timeout: 30_000 });
  const title = /\/title\/series\/tt0944947(\?|$)/;
  const toTitle = async () => {
    // One Escape at a time: each one lands (the address changes) before the
    // next is decided. A second Escape sent while the first was still on its
    // way reached the title page and went on to Home.
    for (let i = 0; i < 3 && !title.test(page.url()); i++) {
      const from = page.url();
      await page.keyboard.press("Escape");
      await expect.poll(() => page.url(), { timeout: 10_000 }).not.toBe(from);
    }
    await expect(page).toHaveURL(title);
  };
  try {
    // Primary action: play this episode with the core's pick for it (the
    // Sample Source's file for S1:E5), not the S2:E3 resume target.
    await row.focus();
    await page.keyboard.press("Enter");
    await expect(page).toHaveURL(/\/play\/series\/tt0944947\/tt0944947(:|%3A)1(:|%3A)5/);
    await expect(page.locator(".ptitle p")).toContainText("S1:E5 The Wolf and the Lion");
    await expect(page.locator('[data-f="player:toggle"]')).toBeVisible({ timeout: 30_000 });
    await startIfPaused(page);
    const time = () => page.evaluate(() => document.querySelector("video")?.currentTime ?? 0);
    await expect.poll(time, { timeout: 30_000 }).toBeGreaterThan(3);
    await toTitle();
    // Back lands on the season being browsed, and the saved progress is S1:E5's.
    await expect(page.locator('[data-f="season:1"]')).toHaveAttribute("aria-selected", "true");
    await expect(page.locator("li.ep.now", { has: row })).toHaveCount(1, { timeout: 15_000 });

    // Secondary action: this episode's Versions, to choose another release.
    await row.focus();
    await page.keyboard.press("ArrowRight");
    await expect.poll(() => focused(page)).toBe(`epv:${SAMPLE_EPISODE}`);
    await page.keyboard.press("Enter");
    await expect(page).toHaveURL(/\/versions\/tt0944947(:|%3A)1(:|%3A)5/);
    await expect(page.locator("h1")).toContainText("S1:E5 The Wolf and the Lion");
    await expect.poll(() => focused(page), { timeout: 30_000 }).toBe("versions:play");
    await toTitle();

    // Secondary action: Mark watched, both ways, by remote.
    await row.focus();
    await page.keyboard.press("ArrowRight");
    await page.keyboard.press("ArrowDown");
    await expect.poll(() => focused(page)).toBe(`mark:${SAMPLE_EPISODE}`);
    const mark = page.locator(`[data-f="mark:${SAMPLE_EPISODE}"]`);
    await page.keyboard.press("Enter");
    await expect(mark).toHaveAttribute("aria-pressed", "true");
    await page.keyboard.press("Enter");
    await expect(mark).toHaveAttribute("aria-pressed", "false");

    // Back lands on the season being browsed even when it holds neither the
    // resume target nor anything watched (season 3 here).
    await page.locator('[data-f="season:3"]').focus();
    await page.keyboard.press("Enter");
    const s3 = page.locator('[data-f="ep:tt0944947:3:1"]');
    await expect(s3).toBeVisible({ timeout: 30_000 });
    await s3.focus();
    await page.keyboard.press("ArrowRight");
    await page.keyboard.press("Enter");
    await expect(page).toHaveURL(/\/versions\/tt0944947(:|%3A)3(:|%3A)1/);
    await toTitle();
    await expect(page.locator('[data-f="season:3"]')).toHaveAttribute("aria-selected", "true");
    await expect(s3).toBeVisible();
  } finally {
    // Put the seeded order back: S2:E3 resumes Game of Thrones, BBB leads.
    const c = new Client();
    await watch(c, { kind: "series", itemId: GOT.id, videoId: GOT.resume, title: "Game of Thrones", season: 2, episode: 3 }, 21 * 60_000, 57 * 60_000);
    await watch(c, { kind: "movie", itemId: BBB_ID, videoId: BBB_ID, title: "Big Buck Bunny", via: "Sample Source" }, 150_000, 596_000);
    c.close();
  }
});

test("Home rows: move by remote, Back drops, hide one, Home follows", async ({ page }) => {
  const titles = () => page.locator(".hrow-text b").allTextContents();
  const homeRows = async () => {
    await page.goto("/?form=tv");
    await expect(page.locator(".row").first()).toHaveAttribute("aria-label", "Continue Watching", { timeout: 30_000 });
    return (await page.locator(".row").evaluateAll((els) => els.map((e) => e.getAttribute("aria-label")))).filter((t) => t !== "Continue Watching");
  };
  try {
    await page.goto("/home-rows?form=tv");
    await expect(page.locator(".hrow-main").first()).toBeVisible({ timeout: 30_000 });
    const before = await titles();
    expect(before.length).toBeGreaterThan(3);
    expect((await homeRows()).slice(0, 3)).toEqual(before.slice(0, 3));
    await page.goto("/home-rows?form=tv");
    await expect(page.locator(".hrow-main").first()).toBeVisible({ timeout: 30_000 });

    // Select picks the first row up; down twice moves it; select drops it.
    await expect.poll(() => focused(page)).toMatch(/^hrow:/);
    const picked = await focused(page);
    await page.keyboard.press("Enter");
    await expect(page.locator(".hrow.moving")).toHaveCount(1);
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("ArrowDown");
    await expect.poll(titles).toEqual([before[1], before[2], before[0], ...before.slice(3)]);
    await expect.poll(() => focused(page)).toBe(picked);
    await page.keyboard.press("Enter");
    await expect(page.locator(".hrow.moving")).toHaveCount(0);

    // Back drops a picked-up row and stays on the screen.
    await page.keyboard.press("Enter");
    await page.keyboard.press("ArrowUp");
    await expect.poll(titles).toEqual([before[1], before[0], before[2], ...before.slice(3)]);
    await page.keyboard.press("Escape");
    await expect(page.locator(".hrow.moving")).toHaveCount(0);
    await expect(page).toHaveURL(/\/home-rows/);

    // Right reaches the row's switch; select hides the row.
    await page.keyboard.press("ArrowRight");
    await expect.poll(() => focused(page)).toBe(`${picked}:shown`);
    await page.keyboard.press("Enter");
    await expect(page.locator(`[data-f="${picked}:shown"]`)).toHaveAttribute("aria-pressed", "false");
    await expect(page.locator(".hrow.off")).toHaveCount(1);

    // Home follows: the hidden row is gone, the order is the new one.
    const now = await homeRows();
    expect(now).not.toContain(before[0]);
    expect(now.slice(0, 2)).toEqual([before[1], before[2]]);
  } finally {
    const c = new Client();
    await c.req("setPrefs", { patch: { homeRows: { order: [], hidden: [] } } });
    c.close();
  }
});

test("Calendar: days in order, today's day first, an episode opens its season", async ({ page }) => {
  const sent: string[] = [];
  page.on("websocket", (ws) => ws.on("framesent", (f) => String(f.payload).includes("setToday") && sent.push(String(f.payload))));
  await page.goto("/calendar?form=tv");
  await expect(page.locator(".cal-card").first()).toBeVisible({ timeout: 60_000 });
  await expect(page.locator(".cal-head .st.warn")).toHaveCount(0, { timeout: 60_000 });

  const days = await page.locator(".cal-day").evaluateAll((els) => els.map((e) => e.getAttribute("data-day") ?? ""));
  expect(days.length).toBeGreaterThan(0);
  expect([...days].sort()).toEqual(days);
  const today = await page.evaluate(() => {
    const d = new Date();
    return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
  });
  const first = days.find((d) => d >= today);
  expect(first).toBeTruthy();
  // The first day on or after today has focus and sits at the top.
  await expect.poll(() => page.evaluate(() => document.activeElement?.closest(".cal-day")?.getAttribute("data-day"))).toBe(first);
  const top = await page.locator(`.cal-day[data-day="${first}"]`).evaluate((el) => el.getBoundingClientRect().top);
  expect(top).toBeLessThan(400);

  // Every item reads series, episode and title.
  const card = page.locator(".cal-card:focus");
  await expect(card.locator(".cal-text b")).not.toHaveText("");
  await expect(card.locator(".cal-ep .num")).toHaveText(/^S\d+:E\d+$/);

  // Foreground sends the local date again.
  await page.evaluate(() => document.dispatchEvent(new Event("visibilitychange")));
  await expect.poll(() => sent.some((t) => t.includes(`"today":"${today}"`))).toBe(true);

  // Another card than today's first, then select: the title on that episode's season.
  const firstKey = await focused(page);
  await page.keyboard.press("ArrowDown");
  if ((await focused(page)) === firstKey) await page.keyboard.press("ArrowUp");
  const opened = await focused(page);
  expect(opened).not.toBe(firstKey);
  const season = await card.evaluate((a) => /season=(\d+)/.exec(a.getAttribute("href") ?? "")?.[1] ?? "");
  expect(season).not.toBe("");
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(new RegExp(`/title/series/[^?]+\\?season=${season}`));
  await expect(page.locator(`[data-f="season:${season}"]`)).toHaveAttribute("aria-selected", "true", { timeout: 30_000 });

  // Back lands on the card that opened it and stays there while the schedule loads again.
  await page.keyboard.press("Escape");
  await expect(page).toHaveURL(/\/calendar/);
  await expect.poll(() => focused(page), { timeout: 30_000 }).toBe(opened);
  await expect(page.locator(".cal-head .st.warn")).toHaveCount(0, { timeout: 60_000 });
  await page.waitForTimeout(1000);
  expect(await focused(page)).toBe(opened);
});

test("Calendar sends the new date at local midnight", async ({ page }) => {
  const now = new Date();
  await page.clock.install({ time: new Date(now.getFullYear(), now.getMonth(), now.getDate(), 23, 59, 55) });
  const sent: string[] = [];
  page.on("websocket", (ws) => ws.on("framesent", (f) => String(f.payload).includes("setToday") && sent.push(String(f.payload))));
  await page.goto("/calendar?form=tv");
  const lede = page.locator(".cal .lede");
  await expect(page.locator(".cal-day").first()).toBeVisible({ timeout: 60_000 });
  const was = await lede.textContent();
  await page.clock.fastForward("00:10");
  const t = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1);
  const tomorrow = `${t.getFullYear()}-${String(t.getMonth() + 1).padStart(2, "0")}-${String(t.getDate()).padStart(2, "0")}`;
  await expect.poll(() => sent.some((s) => s.includes(`"today":"${tomorrow}"`))).toBe(true);
  // The core answers with the new window: the date span moves by a day.
  await expect(lede).not.toHaveText(was ?? "");
});

const FORMS = { tv: [1920, 1080, 1], desktop: [1440, 900, 1], phone: [390, 844, 2] } as const;

for (const [form, [width, height, scale]] of Object.entries(FORMS)) {
  test(`layout rhythm on ${form}: hero gap and one row of title actions`, async ({ browser }) => {
    const context = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: scale, isMobile: form === "phone", hasTouch: form === "phone" });
    const page = await context.newPage();

    // The hero's buttons sit at least as far above "Continue Watching" as a
    // row's content sits above the next row's heading.
    await page.goto(`/?form=${form}`);
    // Continue Watching can land after the catalogs (its tiles wait on metas).
    await expect(page.locator(".row").first()).toHaveAttribute("aria-label", "Continue Watching", { timeout: 30_000 });
    await expect(page.locator(".hero .acts .btn").first()).toBeVisible();
    await expect(page.locator(".row").nth(1).locator(".tile").first()).toBeVisible({ timeout: 30_000 });
    // Focus moves by itself only on TV: pointer and touch get no ring on load.
    if (form === "tv") await expect.poll(() => focused(page)).not.toBeNull();
    else {
      await page.waitForTimeout(500);
      expect(await focused(page)).toBeNull();
    }
    const gaps = await page.evaluate(() => {
      const bottom = (els: Element[]) => Math.max(...els.map((e) => e.getBoundingClientRect().bottom));
      const rows = [...document.querySelectorAll(".row")];
      const heading = (i: number) => rows[i]?.querySelector(".row-head h2")?.getBoundingClientRect().top ?? 0;
      const hero = bottom([...document.querySelectorAll(".hero .acts .btn")]);
      const firstRow = bottom([...(rows[0]?.querySelectorAll(".tile") ?? [])]);
      return { hero: heading(0) - hero, row: heading(1) - firstRow };
    });
    expect(gaps.hero).toBeGreaterThanOrEqual(gaps.row - 1);
    // The hero's eyebrow stays clear of the floating pill, even over a
    // synopsis that runs to its three lines (24 canvas px, 15 at desktop scale).
    if (form !== "phone") {
      const clear = await page.evaluate(() => {
        const syn = document.querySelector(".hero-syn");
        if (syn) syn.textContent = "A long synopsis. ".repeat(40);
        const chip = document.querySelector(".hero .reason")?.getBoundingClientRect().top ?? 0;
        const nav = document.querySelector(".topnav")?.getBoundingClientRect().bottom ?? 0;
        return chip - nav;
      });
      expect(clear).toBeGreaterThan(12);
    }

    // Title actions never leave one button alone on a second row: a movie
    // with a trailer has the most secondary actions.
    await page.goto(`/title/movie/${BBB_ID}?form=${form}`);
    await expect(page.locator('[data-f="title:trailer"]')).toBeVisible({ timeout: 30_000 });
    const row = await page.locator(".info .acts").evaluate((acts) => {
      const box = (acts.closest(".info") as HTMLElement).getBoundingClientRect();
      const buttons = [...acts.querySelectorAll(".btn")].map((e) => e.getBoundingClientRect());
      return {
        tops: new Set(buttons.map((b) => Math.round(b.top))).size,
        // One row that also fits its column: nothing spills past the edge.
        overflow: Math.max(...buttons.map((b) => b.right)) - box.right,
      };
    });
    expect(row.tops).toBe(1);
    expect(row.overflow).toBeLessThanOrEqual(1);
    // Watched is the labeled pill beside Play, not a secondary action. Their layout boxes, not
    // their drawn ones: on TV the focused Play rises by focus.rise.
    const middle = (key: string) =>
      page.locator(`[data-f="${key}"]`).evaluate((el) => {
        let y = (el as HTMLElement).offsetHeight / 2;
        for (let n: HTMLElement | null = el as HTMLElement; n; n = n.offsetParent as HTMLElement | null) y += n.offsetTop;
        return y;
      });
    expect(Math.abs((await middle("title:watched")) - (await middle("title:play")))).toBeLessThan(2);
    // The display class comes from the screen, not the window.
    await page.goto(`/title/series/${GOT.id}/versions/${GOT.resume}?form=${form}`);
    await expect(page.locator(".device")).toContainText(/^This (TV|computer|phone): 1080p,/, { timeout: 30_000 });
    await context.close();
  });
}
