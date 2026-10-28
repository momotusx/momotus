import { expect, test } from "bun:test";
import type { DownloadBadge } from "../core/generated/DownloadBadge";
import type { DownloadReason } from "../core/generated/DownloadReason";
import type { DownloadRow } from "../core/generated/DownloadRow";
import type { DownloadState } from "../core/generated/DownloadState";
import {
  deleteAll,
  deleteSeries,
  downloadControl,
  type DownloadControl,
  type DownloadItem,
  controlName,
  shownDownloads,
  deleteSeason,
  episodeLine,
  episodeName,
  itemOp,
  kidsOf,
  left,
  ofTotal,
  progressLine,
  seriesControl,
  seriesMenu,
  storageLine,
  subtitlesLine,
  titleLine,
} from "./download";

const badge = (state: DownloadState, reason: DownloadReason | null = null, progress: number | null = null): DownloadBadge => ({
  id: "d1",
  state,
  reason,
  mobileData: false,
  progress,
  bytesTotal: 2_000_000_000,
});
const shown = (c: DownloadControl) => [c.glyph, c.label, c.spoken, c.value, c.ring, c.items, c.hint];
const hint = "Shows download options";

test("every state gives its glyph, label, spoken word, value, ring and menu (3.1), as Android's", () => {
  expect(shown(downloadControl(null))).toEqual(["download", "Download", "Download", null, null, [], null]);
  expect(shown(downloadControl(badge("queued")))).toEqual(["clock", "Queued", "Queued", null, 0, ["cancel"], hint]);
  expect(shown(downloadControl(badge("running", null, 0.45)))).toEqual(["stop", "45%", "Downloading", "45 percent", 0.45, ["pause", "cancel"], hint]);
  // No size yet: the word, an empty ring, no value.
  expect(shown(downloadControl(badge("running")))).toEqual(["stop", "Downloading", "Downloading", null, 0, ["pause", "cancel"], hint]);
  expect(shown(downloadControl(badge("paused", null, 0.3)))).toEqual(["pause", "Paused", "Paused", "30 percent", 0.3, ["resume", "cancel"], hint]);
  expect(shown(downloadControl(badge("done", null, 1)))).toEqual(["check", "Downloaded", "Downloaded", null, null, ["play", "delete"], hint]);
  expect(shown(downloadControl(badge("failed", "source")))).toEqual(["alert", "Couldn't download", "Couldn't download", null, null, ["retry", "another", "cancel"], hint]);
  expect(shown(downloadControl(badge("missing")))).toEqual(["alert", "Download again", "Download again", null, null, ["again", "remove"], hint]);
});

test("a waiting download says its reason; only Wi-Fi and Low Data offer mobile data; room is waited for quietly", () => {
  const cases: [DownloadReason, [string, string, DownloadItem[], string | null]][] = [
    ["wifi", ["wifiOff", "Waiting for Wi-Fi", ["mobileData", "cancel"], "Download on Wi-Fi only is on, and this network uses mobile data."]],
    ["lowData", ["wifiOff", "Waiting: Low Data Mode is on", ["mobileData", "cancel"], null]],
    // The owner's rule: never a storage warning. "Waiting", the clock, no detail line.
    ["space", ["clock", "Waiting", ["cancel"], null]],
    ["network", ["wifiOff", "Waiting for a connection", ["cancel"], "It goes on by itself when the connection comes back."]],
    ["playing", ["clock", "Paused while you watch", ["cancel"], "It goes on by itself when playback ends."]],
    ["app", ["clock", "Opens again when you return to Momotus", ["cancel"], null]],
  ];
  for (const [reason, want] of cases) {
    const c = downloadControl(badge("waiting", reason, 0.2));
    expect([c.glyph, c.label, c.items, c.detail]).toEqual(want);
    expect(c.spoken).toBe(c.label);
    expect(c.value).toBe("20 percent");
    expect(c.ring).toBe(0.2);
  }
});

test("each item sends its op, and mobile data releases a waiting one", () => {
  expect(itemOp("pause")).toEqual({ op: "downloadPause", mobileData: false });
  expect(itemOp("resume")).toEqual({ op: "downloadResume", mobileData: false });
  for (const gone of ["cancel", "delete", "remove"] as const) expect(itemOp(gone)).toEqual({ op: "downloadRemove", mobileData: false });
  expect(itemOp("retry")).toEqual({ op: "downloadRetry", mobileData: false });
  expect(itemOp("again")).toEqual({ op: "downloadRetry", mobileData: false });
  expect(itemOp("mobileData")).toEqual({ op: "downloadRetry", mobileData: true });
  for (const elsewhere of ["start", "choose", "another", "play"] as const) expect(itemOp(elsewhere)).toBeNull();
});

test("a failure says why in full", () => {
  expect((["noVersion", "source", "retries", "notVideo"] as const).map((r) => downloadControl(badge("failed", r)).detail)).toEqual([
    "No version of this one can be downloaded",
    "The source stopped answering",
    "The download was interrupted too many times",
    "The file wasn't a video",
  ]);
});

test("the series menu counts aired episodes and says where the season stands", () => {
  const series = { seasons: 3, episodes: 28, downloaded: 0, active: 0 };
  expect(seriesMenu(1, { seasons: 1, episodes: 12, downloaded: 0, active: 0 }, series)).toEqual([
    { label: "Download Season 1 (12 episodes)", act: "season" },
    { label: "Download every season (3 seasons, 28 episodes)", act: "series" },
  ]);
  expect(seriesMenu(1, { seasons: 1, episodes: 12, downloaded: 11, active: 0 }, { ...series, downloaded: 11 })).toEqual([
    { label: "Download Season 1 (1 episode)", act: "season" },
    { label: "Delete Season 1 downloads", act: "deleteSeason" },
    { label: "Download every season (3 seasons, 28 episodes)", act: "series" },
  ]);
  expect(seriesMenu(2, { seasons: 1, episodes: 8, downloaded: 8, active: 3 }, series)[0]).toEqual({ label: "Downloading Season 2", act: null });
  expect(seriesMenu(2, { seasons: 1, episodes: 8, downloaded: 8, active: 0 }, series)[0]).toEqual({ label: "Season 2 downloaded", act: null });
  // One season, or everything kept: no "every season".
  expect(seriesMenu(1, { seasons: 1, episodes: 8, downloaded: 0, active: 0 }, { seasons: 1, episodes: 8, downloaded: 0, active: 0 })).toEqual([{ label: "Download Season 1 (8 episodes)", act: "season" }]);
  expect(shown(seriesControl(null))).toEqual(["download", "Download", "Download", null, null, [], hint]);
  expect(seriesControl({ ...series, downloaded: 3, active: 1 }).label).toBe("Downloading");
  // One episode of 28 kept is not the series downloaded: it still reads Download, its arrow.
  expect(shown(seriesControl({ ...series, downloaded: 1 }))).toEqual(["download", "Download", "Download", null, null, [], hint]);
  expect(shown(seriesControl({ ...series, downloaded: 28 }))).toEqual(["check", "Downloaded", "Downloaded", null, null, [], hint]);
});

test("the Downloads screen's lines", () => {
  const gb = 1024 ** 3;
  expect(storageLine({ downloadsBytes: Math.floor(12.4 * gb), freeBytes: 48 * gb, neededBytes: null })).toBe("Downloads use 12.4 GB · 48 GB free on this device");
  expect(storageLine({ downloadsBytes: 300 * 1024 ** 2, freeBytes: null, neededBytes: 5 * gb })).toBe("Downloads use 300 MB");
  expect(ofTotal(Math.floor(1.2 * gb), Math.floor(2.1 * gb))).toBe("1.2 of 2.1 GB");
  expect(ofTotal(300 * 1024 ** 2, Math.floor(2.1 * gb))).toBe("300 MB of 2.1 GB");
  expect(left(330)).toBe("6 min left");
  expect(left(3_900)).toBe("1 h 5 min left");
  expect(left(20)).toBe("Less than a minute left");
  const row: DownloadRow = {
    id: "d1",
    kind: "series",
    itemId: "tt1",
    videoId: "tt1:1:3",
    season: 1,
    episode: 3,
    name: "Severance",
    episodeTitle: "In Perpetuity",
    poster: null,
    still: null,
    runtime: "52 min",
    released: null,
    state: "running",
    reason: null,
    mobileData: false,
    bytesDone: Math.floor(1.2 * gb),
    bytesTotal: Math.floor(2.1 * gb),
    speedBps: null,
    etaSecs: 330,
    quality: "1080p",
    subtitles: [],
  };
  expect(progressLine(row)).toBe("1.2 of 2.1 GB · 6 min left");
  expect(progressLine({ ...row, state: "waiting", reason: "wifi" })).toBe("1.2 of 2.1 GB · Waiting for Wi-Fi");
  expect(progressLine({ ...row, state: "waiting", reason: "space" })).toBe("1.2 of 2.1 GB · Waiting");
  expect(episodeName(row)).toBe("E3 In Perpetuity");
  expect(episodeLine(row)).toBe("52 min · 2.1 GB");
  const film = { itemId: "tt2", kind: "movie", name: "Sintel", poster: null, count: 1, bytes: Math.floor(2.1 * gb), rows: [{ ...row, kind: "movie" }] };
  expect(titleLine(film)).toBe("1080p · 2.1 GB");
  expect(titleLine({ ...film, kind: "series", count: 4 })).toBe("4 episodes · 2.1 GB");
  expect(deleteSeries(4, "Severance")).toBe("Delete 4 downloaded episodes of Severance?");
  expect(deleteSeries(1, "Severance")).toBe("Delete 1 downloaded episode of Severance?");
  expect(deleteAll(12 * gb)).toBe("Delete all downloads on this device? 12 GB");
});

test("a control's name starts with what it shows, then its state, its value and what it downloads", () => {
  expect(controlName(downloadControl(null), "Big Buck Bunny")).toBe("Download, Big Buck Bunny");
  expect(controlName(downloadControl(badge("running", null, 0.45)), "S1:E2")).toBe("45%, Downloading, S1:E2");
  expect(controlName(downloadControl(badge("running")), "S1:E2")).toBe("Downloading, S1:E2");
  expect(controlName(downloadControl(badge("paused", null, 0.3)), "Big Buck Bunny")).toBe("Paused, 30 percent, Big Buck Bunny");
  expect(controlName(downloadControl(badge("waiting", "wifi", 0.2)), "S2:E1 Pilot")).toBe("Waiting for Wi-Fi, 20 percent, S2:E1 Pilot");
  expect(controlName(downloadControl(badge("done", null, 1)), "Big Buck Bunny")).toBe("Downloaded, Big Buck Bunny");
});

test("a profile not yet known, or not found, gets no download controls, as a kids one", () => {
  const p = (kids: boolean, active = "a") => ({ profiles: [{ id: "a", name: "A", avatar: "letter:sky", position: 0, hasPin: false, isOwner: true, syncing: false, kids }], active, locked: false, max: 6, kids, wipePending: null });
  expect(kidsOf(null)).toBe(true);
  expect(kidsOf(undefined)).toBe(true);
  expect(kidsOf(p(false, "gone"))).toBe(true);
  expect(kidsOf(p(true))).toBe(true);
  expect(kidsOf(p(false))).toBe(false);
});

test("the subtitles kept with a download, by language name", () => {
  expect(subtitlesLine(["en", "tr"])).toBe("Subtitles: English, Turkish");
  expect(subtitlesLine([])).toBeNull();
});

test("a season's delete counts the episodes already downloaded, never the queued ones it also cancels", () => {
  const b = (state: DownloadState) => badge(state);
  const season = [b("done"), b("done"), b("done"), ...Array.from({ length: 9 }, () => b("queued"))];
  expect(deleteSeason(2, season, "Severance")).toBe("Delete 3 downloaded episodes of Severance?");
  expect(deleteSeason(2, [b("done"), b("running")], "Severance")).toBe("Delete 1 downloaded episode of Severance?");
  // Nothing done yet: the season's downloads, with no count that is not true.
  expect(deleteSeason(2, [b("queued"), b("running"), null], "Severance")).toBe("Delete Season 2 downloads of Severance?");
  // A whole series (its Downloads page): the same count, and no season named.
  expect(deleteSeason(null, [b("done"), ...Array.from({ length: 9 }, () => b("queued"))], "Severance")).toBe("Delete 1 downloaded episode of Severance?");
  expect(deleteSeason(null, [b("queued")], "Severance")).toBe("Delete the downloads of Severance?");
});

test("a locked session's counts-only snapshot is never shown, so no screen reads it as empty", () => {
  const open = { downloading: [], titles: [], locked: false };
  expect(shownDownloads(open)).toBe(open);
  expect(shownDownloads({ ...open, locked: true })).toBeNull();
  expect(shownDownloads(null)).toBeNull();
  // A core from before the field: shown as it always was.
  expect(shownDownloads({ downloading: [], titles: [] })).toEqual({ downloading: [], titles: [] });
});
