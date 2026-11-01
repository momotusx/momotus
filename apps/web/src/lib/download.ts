/** The words of downloads (plans/offline-downloads.md 3.1, 3.4, 3.9), equal to Android's
 * DownloadCopy.kt and Apple's: what the control shows for one video, its menu, the series menu
 * and the Downloads screen's lines. Pure, so one test pins every state and reason. */

import type { DownloadBadge } from "../core/generated/DownloadBadge";
import type { DownloadReason } from "../core/generated/DownloadReason";
import type { DownloadRow } from "../core/generated/DownloadRow";
import type { DownloadStorage } from "../core/generated/DownloadStorage";
import type { DownloadTitle } from "../core/generated/DownloadTitle";
import type { GroupDownload } from "../core/generated/GroupDownload";
import type { ProfilesSnapshot } from "../core/generated/ProfilesSnapshot";
import { bytes } from "./format";
import { languageName } from "./words";
import { current } from "./profiles";

/** The control's Lucide glyph: `download` is arrow-down-to-line, `stop` the square in the ring. */
export type DownloadGlyph = "download" | "stop" | "pause" | "wifiOff" | "clock" | "check" | "alert";

/** One item of a download's menu. */
export type DownloadItem = "start" | "choose" | "pause" | "resume" | "cancel" | "delete" | "play" | "retry" | "another" | "mobileData" | "again" | "remove";

export const ITEM_LABEL: Record<DownloadItem, string> = {
  start: "Download",
  choose: "Choose a version to download",
  pause: "Pause download",
  resume: "Resume download",
  cancel: "Cancel download",
  delete: "Delete download",
  play: "Play",
  retry: "Try again",
  another: "Choose another version",
  mobileData: "Download now on mobile data",
  again: "Download again",
  remove: "Remove",
};

/** What the control shows for one video: `label` beside or under it ("45%"), `spoken` the state's
 * word for a screen reader ("Downloading"), `value` the ring's "45 percent", `ring` its fill (null
 * with no ring), `detail` the reason in full for the menu, `items` the menu a press opens (empty:
 * a press starts the download), `hint` what that press does. */
export interface DownloadControl {
  glyph: DownloadGlyph;
  label: string;
  spoken: string;
  value: string | null;
  ring: number | null;
  detail: string | null;
  items: DownloadItem[];
  hint: string | null;
}

const MENU_HINT = "Shows download options";

const control = (glyph: DownloadGlyph, label: string, spoken: string, value: string | null, ring: number | null, detail: string | null, items: DownloadItem[]): DownloadControl => ({
  glyph,
  label,
  spoken,
  value,
  ring,
  detail,
  items,
  hint: items.length ? MENU_HINT : null,
});

/** Where nothing is downloaded yet: a press starts it. */
export const NONE = control("download", "Download", "Download", null, null, null, []);

/** The waiting label by its reason. A wait for room is never a storage warning (the owner's rule):
 * it reads "Waiting", quietly. */
export function waiting(reason: DownloadReason | null): string {
  switch (reason) {
    case "wifi":
      return "Waiting for Wi-Fi";
    case "lowData":
      return "Waiting: Low Data Mode is on";
    case "network":
      return "Waiting for a connection";
    case "playing":
      return "Paused while you watch";
    case "app":
      return "Opens again when you return to Momotus";
    default:
      return "Waiting";
  }
}

const waitingGlyph = (reason: DownloadReason | null): DownloadGlyph => (reason === "wifi" || reason === "lowData" || reason === "network" ? "wifiOff" : "clock");

function waitingDetail(reason: DownloadReason | null): string | null {
  switch (reason) {
    case "wifi":
      return "Download on Wi-Fi only is on, and this network uses mobile data.";
    case "network":
      return "It goes on by itself when the connection comes back.";
    case "playing":
      return "It goes on by itself when playback ends.";
    default:
      return null;
  }
}

/** Why a download failed, in full. */
export function failure(reason: DownloadReason | null): string {
  switch (reason) {
    case "noVersion":
      return "No version of this one can be downloaded";
    case "source":
      return "The source stopped answering";
    case "retries":
      return "The download was interrupted too many times";
    case "notVideo":
      return "The file wasn't a video";
    default:
      return "Something went wrong";
  }
}

/** A video's control: `badge` null is not downloaded. */
export function downloadControl(badge: DownloadBadge | null | undefined): DownloadControl {
  if (!badge) return NONE;
  const progress = badge.progress === null ? null : Math.min(1, Math.max(0, badge.progress));
  const percent = progress === null ? null : String(Math.round(progress * 100));
  const value = percent === null ? null : `${percent} percent`;
  switch (badge.state) {
    case "queued":
      return control("clock", "Queued", "Queued", null, 0, null, ["cancel"]);
    case "running":
      return control("stop", percent === null ? "Downloading" : `${percent}%`, "Downloading", value, progress ?? 0, null, ["pause", "cancel"]);
    case "paused":
      return control("pause", "Paused", "Paused", value, progress ?? 0, null, ["resume", "cancel"]);
    case "waiting": {
      const word = waiting(badge.reason);
      const release = badge.reason === "wifi" || badge.reason === "lowData";
      return control(waitingGlyph(badge.reason), word, word, value, progress ?? 0, waitingDetail(badge.reason), release ? ["mobileData", "cancel"] : ["cancel"]);
    }
    case "done":
      return control("check", "Downloaded", "Downloaded", null, null, null, ["play", "delete"]);
    case "failed":
      return control("alert", "Couldn't download", "Couldn't download", null, null, failure(badge.reason), ["retry", "another", "cancel"]);
    case "missing":
      return control("alert", "Download again", "Download again", null, null, "The downloaded file is no longer on this device.", ["again", "remove"]);
    default:
      // A state from a newer core: it can still be cancelled.
      return control("clock", "Waiting", "Waiting", null, 0, null, ["cancel"]);
  }
}

export type DownloadOp = "downloadPause" | "downloadResume" | "downloadRemove" | "downloadRetry";

/** The op an item of an existing download sends, and whether it releases the download onto mobile
 * data; null for the items that go somewhere instead (Download, a version, Play). */
export function itemOp(item: DownloadItem): { op: DownloadOp; mobileData: boolean } | null {
  switch (item) {
    case "pause":
      return { op: "downloadPause", mobileData: false };
    case "resume":
      return { op: "downloadResume", mobileData: false };
    case "cancel":
    case "delete":
    case "remove":
      return { op: "downloadRemove", mobileData: false };
    case "retry":
    case "again":
      return { op: "downloadRetry", mobileData: false };
    case "mobileData":
      return { op: "downloadRetry", mobileData: true };
    default:
      return null;
  }
}

export const episodes = (n: number) => (n === 1 ? "1 episode" : `${n} episodes`);

export type GroupAct = "season" | "series" | "deleteSeason" | null;

/** A series' Download menu (3.2): the selected season's item, then every season's. Counts are aired
 * regular episodes, from the core; an item with no act only says where the season stands. */
export function seriesMenu(season: number | null, group: GroupDownload | null, series: GroupDownload | null): { label: string; act: GroupAct }[] {
  const out: { label: string; act: GroupAct }[] = [];
  if (season !== null && group && group.episodes > 0) {
    const left = group.episodes - group.downloaded;
    if (group.active > 0) out.push({ label: `Downloading Season ${season}`, act: null });
    else if (left <= 0) out.push({ label: `Season ${season} downloaded`, act: null });
    else out.push({ label: `Download Season ${season} (${episodes(left)})`, act: "season" });
    if (group.downloaded > 0) out.push({ label: `Delete Season ${season} downloads`, act: "deleteSeason" });
  }
  if (series && series.seasons > 1 && series.downloaded < series.episodes) {
    out.push({ label: `Download every season (${series.seasons} seasons, ${episodes(series.episodes)})`, act: "series" });
  }
  return out;
}

/** The series' action-row control: Downloading while any is on its way, Downloaded once every aired
 * episode is kept, else Download; a press always opens its menu. */
export function seriesControl(series: GroupDownload | null): DownloadControl {
  const c = series && series.active > 0 ? control("clock", "Downloading", "Downloading", null, null, null, []) : series && series.episodes > 0 && series.downloaded >= series.episodes ? control("check", "Downloaded", "Downloaded", null, null, null, []) : NONE;
  return { ...c, hint: MENU_HINT };
}

/** A control's accessible name: what it shows first (voice control says that), then the state's word,
 * the ring's value and what it downloads, each once: "45%, Downloading, S1:E2", "Paused, 30 percent,
 * Big Buck Bunny". A percent already shown is not said twice. */
export function controlName(c: DownloadControl, subject: string): string {
  const parts = [c.label, c.spoken, c.label.endsWith("%") ? null : c.value, subject];
  return parts.filter((p, i): p is string => !!p && parts.indexOf(p) === i).join(", ");
}

/** Downloads are offered only to a profile known not to be a kids one: before the profiles answer, or
 * with the active profile not among them, it is treated as a kids profile (Play only). Incognito is not one. */
export function kidsOf(s: ProfilesSnapshot | null | undefined): boolean {
  return current(s)?.kids !== false;
}

/** The subtitles kept with a download, by the app's language names: "Subtitles: English, Turkish". */
export const subtitlesLine = (tags: readonly string[]) => (tags.length ? `Subtitles: ${tags.map(languageName).join(", ")}` : null);

/** "Downloads use 12.4 GB · 48 GB free on this device". */
export const storageLine = (s: DownloadStorage) => [`Downloads use ${bytes(s.downloadsBytes)}`, s.freeBytes === null ? null : `${bytes(s.freeBytes)} free on this device`].filter(Boolean).join(" · ");

/** "1.2 of 2.1 GB": the first figure drops the unit both share. */
export function ofTotal(done: number, total: number): string {
  const a = bytes(done);
  const b = bytes(total);
  const unit = (s: string) => s.slice(s.lastIndexOf(" ") + 1);
  return unit(a) === unit(b) ? `${a.slice(0, a.lastIndexOf(" "))} of ${b}` : `${a} of ${b}`;
}

/** "6 min left", "1 h 5 min left"; under a minute "Less than a minute left". */
export function left(seconds: number): string {
  if (seconds < 60) return "Less than a minute left";
  const minutes = Math.ceil(seconds / 60);
  return minutes < 60 ? `${minutes} min left` : `${Math.floor(minutes / 60)} h ${minutes % 60} min left`;
}

/** A row's badge, for the control on the Downloads screen. */
export const badgeOf = (r: DownloadRow): DownloadBadge => ({
  id: r.id,
  state: r.state,
  reason: r.reason,
  mobileData: r.mobileData,
  progress: r.bytesTotal ? r.bytesDone / r.bytesTotal : null,
  bytesTotal: r.bytesTotal,
});

/** A row of the Downloading section: "1.2 of 2.1 GB · 6 min left", or the state's word where it does not run. */
export function progressLine(r: DownloadRow): string {
  const size = r.bytesTotal ? ofTotal(r.bytesDone, r.bytesTotal) : r.bytesDone > 0 ? bytes(r.bytesDone) : null;
  const state = r.state === "running" ? (r.etaSecs === null ? null : left(r.etaSecs)) : downloadControl(badgeOf(r)).label;
  return [size, state].filter(Boolean).join(" · ");
}

/** A kept title's line: a film "1080p · 2.1 GB", a series "4 episodes · 7.8 GB". */
export const titleLine = (t: DownloadTitle) => (t.kind === "movie" ? [t.rows[0]?.quality, bytes(t.bytes)].filter(Boolean).join(" · ") : `${episodes(t.count)} · ${bytes(t.bytes)}`);

/** An episode of a series page: "E3 Name". */
export const episodeName = (r: DownloadRow) => [r.episode === null ? null : `E${r.episode}`, r.episodeTitle?.trim() || null].filter(Boolean).join(" ") || r.name;

/** Under it: "52 min · 1.9 GB". */
export const episodeLine = (r: DownloadRow) => {
  const size = r.bytesTotal ?? r.bytesDone;
  return [r.runtime, size > 0 ? bytes(size) : null].filter(Boolean).join(" · ");
};

export const deleteSeries = (count: number, name: string) => `Delete ${count === 1 ? "1 downloaded episode" : `${count} downloaded episodes`} of ${name}?`;

/** A season's or a series' delete question (no season): the episodes already downloaded, never the
 * queued or running ones it also cancels; with none done yet, the downloads by name. */
export function deleteSeason(season: number | null, badges: readonly (DownloadBadge | null)[], name: string): string {
  const done = badges.filter((b) => b?.state === "done").length;
  if (done > 0) return deleteSeries(done, name);
  return `Delete ${season === null ? "the" : `Season ${season}`} downloads of ${name}?`;
}
export const deleteAll = (n: number) => `Delete all downloads on this device? ${bytes(n)}`;

export const downloadWords = {
  emptyTitle: "Nothing downloaded yet",
  emptyBody: "Download movies and episodes to watch them without a connection.",
  /** The offline banner's constant, the only words it announces (3.5). */
  offlineAlone: "You're offline.",
  /** Said after it when this profile has downloads; never announced. */
  offlineRest: "Your downloads still play.",
  offlineAction: "Go to Downloads",
  titleOffline: "Showing what's downloaded",
  smart: "Smart downloads",
  smartLine: "When you finish a downloaded episode, the next one downloads and the one you watched is deleted.",
  pick: "Pick the version to keep on this device",
  pickAction: "Download this version",
};

/** The snapshot a screen shows: none while the session is locked. A locked session's `downloads` view
 * carries counts only (no titles, nothing done or failed), so a screen still up for a frame as the
 * lock lands reads it as still opening: no "Nothing downloaded yet", no series page that thinks its
 * last episode went. */
export const shownDownloads = <S extends object>(s: S | null | undefined): S | null => (s && !(s as { locked?: boolean }).locked ? s : null);

/** The shell's offline banner (plans/offline-downloads.md 3.5), on every screen but TV's and the
 * player's: whether it says the downloads still play after "You're offline.", and whether it offers
 * the way to Downloads, which it does only when this profile has a done download and the page is not
 * Downloads itself. Null online. */
export function offlineBanner(online: boolean, form: string, path: string, hasDone: boolean): { rest: boolean; action: boolean } | null {
  if (online || form === "tv" || path.startsWith("/play/")) return null;
  return { rest: hasDone, action: hasDone && !path.startsWith("/library/downloads") };
}
