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
  await expect.poll(() => focused(page)).toMatch(/^tile:home:com\.linvo\.cinemeta:/);
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
