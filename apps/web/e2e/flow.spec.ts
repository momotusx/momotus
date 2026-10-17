import { expect, type Page, test } from "@playwright/test";
import { GOT, startIfPaused } from "./core";
import { BBB_ID } from "./source";

test.use({ viewport: { width: 1920, height: 1080 } });

const focused = (page: Page) => page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f ?? null);
const cw = (id: string) => `tile:home:continue-watching:${id}`;

test("remote flow: Home, Title, seasons, Versions and Back restore focus", async ({ page }) => {
  await page.goto("/?form=tv");

  // Continue Watching is the first row and takes the initial focus.
  await expect(page.locator(`[data-f="${cw(BBB_ID)}"]`)).toBeVisible();
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
  await expect(page.locator(".title-logo")).toHaveAttribute("alt", "Game of Thrones");
  await expect(page.locator(".info .meta")).toContainText("IMDb");
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

  // Subtitles from OpenSubtitles load into the video as a text track, all by
  // remote: the first press only wakes the hidden controls.
  await page.waitForTimeout(4500);
  await expect(page.locator(".player-scr")).toHaveClass(/idle/);
  await page.keyboard.press("ArrowRight");
  await expect(page.locator(".player-scr")).not.toHaveClass(/idle/);
  await expect.poll(() => focused(page)).toBe("player:toggle");
  for (let i = 0; i < 4 && (await focused(page)) !== "player:subs"; i++) await page.keyboard.press("ArrowRight");
  await expect.poll(() => focused(page)).toBe("player:subs");
  await page.keyboard.press("Enter");
  await expect.poll(() => focused(page)).toBe("sub:off");
  await expect(page.locator('[data-f="sub:0"]')).toBeVisible({ timeout: 30_000 });
  await page.keyboard.press("ArrowDown");
  await expect.poll(() => focused(page)).toBe("sub:0");
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

test("Skip credits from live skip data, by remote, only on a press", async ({ page }) => {
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

  // TheIntroDB marks this release's credits from 8:10.458 to 9:40.917; the
  // core looks them up once the player reports the file's length.
  await seekTo(493);
  await expect(skip).toHaveText("Skip credits", { timeout: 30_000 });
  await expect.poll(() => focused(page)).toBe("player:skip");
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
  await expect.poll(time).toBeGreaterThanOrEqual(580.9);
  await expect(skip).toHaveCount(0);
  await expect.poll(() => focused(page)).toBe("player:toggle");

  // With the controls up (the press woke them) it sits above the progress bar.
  await seekTo(493);
  await expect(skip).toBeVisible();
  await expect(page.locator(".player-scr")).not.toHaveClass(/idle/);
  await expect.poll(async () => (await box(".scrub")).top - (await box('[data-f="player:skip"]')).bottom).toBeGreaterThan(16);
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
  await page.keyboard.press("ArrowLeft");
  await expect.poll(() => focused(page)).toBe("trailer:close");
  // Back closes it and focus returns to the button.
  await page.keyboard.press("Escape");
  await expect(page.locator(".trailer")).toHaveCount(0);
  await expect(page).toHaveURL(new RegExp(`/title/movie/${BBB_ID}`));
  await expect.poll(() => focused(page)).toBe("title:trailer");

  // Mark watched toggles the movie's own flag in the core, both ways.
  const watched = page.locator('[data-f="title:watched"]');
  await expect(watched).toHaveAttribute("aria-pressed", "false");
  await watched.focus();
  await page.keyboard.press("Enter");
  await expect(watched).toHaveAttribute("aria-pressed", "true");
  await expect(watched).toHaveText("Watched");
  await page.keyboard.press("Enter");
  await expect(watched).toHaveAttribute("aria-pressed", "false");
  await expect(watched).toHaveText("Mark watched");
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
    const gaps = await page.evaluate(() => {
      const bottom = (els: Element[]) => Math.max(...els.map((e) => e.getBoundingClientRect().bottom));
      const rows = [...document.querySelectorAll(".row")];
      const heading = (i: number) => rows[i]?.querySelector(".row-head h2")?.getBoundingClientRect().top ?? 0;
      const hero = bottom([...document.querySelectorAll(".hero .acts .btn")]);
      const firstRow = bottom([...(rows[0]?.querySelectorAll(".tile") ?? [])]);
      return { hero: heading(0) - hero, row: heading(1) - firstRow };
    });
    expect(gaps.hero).toBeGreaterThanOrEqual(gaps.row - 1);

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
    // Watched is the round toggle beside Play, not a secondary action.
    const play = await page.locator('[data-f="title:play"]').boundingBox();
    const toggle = await page.locator('[data-f="title:watched"]').boundingBox();
    expect(Math.abs((toggle?.y ?? 0) + (toggle?.height ?? 0) / 2 - ((play?.y ?? 0) + (play?.height ?? 0) / 2))).toBeLessThan(2);
    await context.close();
  });
}
