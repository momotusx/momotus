/** Plain-language copy for the core's typed facts. The core sends enums and
 * codes; every UI turns them into the same words. */

import type { Group } from "../core/generated/Group";
import type { HiddenSummary } from "../core/generated/HiddenSummary";
import type { HideReason } from "../core/generated/HideReason";
import type { Reason } from "../core/generated/Reason";
import type { SubtitleNote } from "../core/generated/SubtitleNote";
import type { SyncReport } from "../core/generated/SyncReport";
import type { Tile } from "../core/generated/Tile";
import type { VersionStatus } from "../core/generated/VersionStatus";
import type { BlockedReason } from "../core/generated/BlockedReason";
import type { LearnedChoice } from "../core/generated/LearnedChoice";
import type { Network } from "../core/generated/Network";
import type { Prefs } from "../core/generated/Prefs";
import type { ReasonLine } from "../core/generated/ReasonLine";
import type { Speed } from "../core/generated/Speed";
import type { StreamPrefs } from "../core/generated/StreamPrefs";
import type { VersionsConditions } from "../core/generated/VersionsConditions";
import type { Form } from "../form";
import { bytes } from "./format";
import choice from "../../../shared/stream-choice-words.json";

export function reasonText(r: Reason, status: VersionStatus): string {
  switch (r) {
    case "readyNow":
      return status.kind === "ready" && status.service ? `Ready now, cached on ${status.service}` : "Ready now";
    case "playsDirectly":
      return "Plays directly on this device";
    case "matchesScreen":
      return "Matches your screen";
    case "sameReleaseAsLast":
      return "Same release as last time";
    case "yourLanguage":
      return "In your language";
    case "smallerFaster":
      return "Smaller and faster to start";
  }
}

export const groupLabel: Record<Group, string> = {
  best: "Best quality",
  smallerFaster: "Smaller and faster",
  otherLanguages: "Other languages",
  needsDownload: "Needs download",
};

export const groupOrder: Group[] = ["best", "smallerFaster", "otherLanguages", "needsDownload"];

const hideText: Record<HideReason, [one: string, many: string]> = {
  wrongTitle: ["is for another title", "are for other titles"],
  wrongEpisode: ["is another episode", "are other episodes"],
  excluded: ["matches your avoid list", "match your avoid list"],
  tooLarge: ["is over your size limit", "are over your size limit"],
  unplayable: ["cannot play on this device", "cannot play on this device"],
  unsupportedSource: ["uses a source this app cannot open yet", "use a source this app cannot open yet"],
  // Kids profiles only.
  leavesApp: ["would open outside Marquee", "would open outside Marquee"],
  notForKids: ["is not for kids", "are not for kids"],
  needsDebrid: ["needs a paid streaming service account", "need a paid streaming service account"],
  noSpace: ["is larger than the free space", "are larger than the free space"],
};

/** "19 are for other titles, 1 is another episode". */
export function hiddenSummary(h: HiddenSummary): string {
  return h.byReason.map(([reason, n]) => `${n} ${hideText[reason][n === 1 ? 0 : 1]}`).join(", ");
}

export type Tone = "ok" | "warn" | "bad" | "idle";

/** Status in words with a tone for its dot; the word always carries it. */
export function statusText(s: VersionStatus): { tone: Tone; text: string } {
  switch (s.kind) {
    case "ready":
      return { tone: "ok", text: s.service ? `Ready on ${s.service}` : "Ready" };
    case "direct":
      return { tone: "ok", text: "Direct link" };
    case "torrent":
      return { tone: "warn", text: s.seeds === null ? "Torrent" : `Torrent, ${s.seeds} seeds` };
    case "needsDownload":
      return { tone: "warn", text: s.service ? `Needs download on ${s.service}` : "Needs download" };
  }
}

const names = (() => {
  try {
    return new Intl.DisplayNames(["en"], { type: "language" });
  } catch {
    return null;
  }
})();

/** "fr" or "fre" to "French"; unknown codes stay as given, upper-cased. */
export function languageName(code: string): string {
  const c = code.toLowerCase();
  const iso1: Record<string, string> = { eng: "en", tur: "tr", fre: "fr", fra: "fr", ger: "de", deu: "de", spa: "es", ita: "it", por: "pt", rus: "ru", jpn: "ja", kor: "ko", chi: "zh", zho: "zh", ara: "ar", dut: "nl", nld: "nl", pol: "pl", swe: "sv", hin: "hi", gre: "el", ell: "el", rum: "ro", ron: "ro", hun: "hu", cze: "cs", ces: "cs", heb: "he", per: "fa", fas: "fa", pob: "pt-BR" };
  try {
    const name = names?.of(iso1[c] ?? c);
    if (name && name.toLowerCase() !== c) return name;
  } catch {
    // Not a language code: fall through.
  }
  return code.toUpperCase();
}

/** A subtitle source that gave nothing this time, as every app says it
 * (plans/m13-turkcealtyazi.md 4.2). */
export function subtitleNote({ source, problem }: SubtitleNote): string {
  switch (problem) {
    case "unreachable":
      return `${source} did not answer. Try its subtitles again in a moment.`;
    case "blocked":
      return `${source} is turning requests away. Marquee will try again in a few minutes.`;
    case "changed":
      return `${source} changed its pages, so Marquee cannot read them. An update will fix this.`;
    case "trackFailed":
      return `A subtitle file from ${source} could not be read. Pick another one.`;
  }
}

/** Continue Watching tiles (in progress, or Next Up) carry their one caption
 * from the core ("S2:E3 · 36 min left", "Next: S1:E2"); it is shown as is.
 * Catalog tiles show none. */
export const caption = (tile: Tile): string | null =>
  tile.progress !== null || tile.releaseInfo?.startsWith("Next:") ? tile.releaseInfo : null;

/** A version's headline as the core words it: the resolution, else the
 * source ("WEB-DL"), else "Quality not stated"; never "unknown". */
export const qualityText = (q: string) => (q === "Unknown" ? "Quality not stated" : q);
/** The same fact in a table cell or a small value: one word that never wraps. */
export const qualityCell = (q: string) => (q === "Unknown" || q === "Quality not stated" ? "Unknown" : q);

export function kindLabel(kind: string): string {
  const labels: Record<string, string> = { movie: "Movie", series: "Series", channel: "Channel", tv: "Live TV", anime: "Anime" };
  return labels[kind] ?? kind.charAt(0).toUpperCase() + kind.slice(1);
}

/** What an addon provides, in words. */
export function providesText(p: string): string {
  const labels: Record<string, string> = {
    catalog: "Catalogs",
    meta: "Details",
    stream: "Streams",
    subtitles: "Subtitles",
    addon_catalog: "Addon lists",
  };
  return labels[p] ?? p;
}

/**
 * Why pairing stopped, in words. The core's own pairing reasons are fixed
 * sentences and are kept; a connection or address error is replaced, since its
 * text can name the server's address (Android's `SyncCopy.pairingProblem`).
 */
export function pairingProblem(reason: string): string {
  if (reason.startsWith("pairing: ")) {
    const why = reason.slice("pairing: ".length);
    return `${why.charAt(0).toUpperCase()}${why.slice(1)}.`;
  }
  if (reason.startsWith("the relay must use https")) return "The sync server address must start with https.";
  if (reason.startsWith("invalid url")) return "That is not a sync server address. It looks like https://sync.example.org.";
  if (reason.startsWith("blocked private address")) return "This device does not connect to that address.";
  return "The sync server did not answer. Check the address and try again.";
}

/** The server's host name only: never a path, a query or a user name. */
export function serverHost(url: string): string {
  try {
    return new URL(url).hostname || "the sync server";
  } catch {
    return "the sync server";
  }
}

/** An address with a host and no user name or password in it. */
export function plainAddress(url: string): boolean {
  try {
    const u = new URL(url);
    return u.hostname !== "" && u.username === "" && u.password === "";
  } catch {
    return false;
  }
}

/** Find addons copy, word for word the same as Android's `FindAddonsCopy`
 * and Apple's `FindAddonsWords` (each is tested against
 * `apps/shared/find-addons-words.json`). `{kind}` and `{host}` are filled in. */
/** A pasted link whose settings carry a sign-in (`tokenInLink`). */
/** Where an addon runs, shown instead of its host: the core reads a
 * built-in source itself (`AddonInfo.builtIn`). */
export const addonHost = (a: { builtIn: boolean; host: string }) => (a.builtIn ? "Built into Marquee, runs on this device" : a.host);

/** Settings, About: the built-in source's credit (plans/m13-turkcealtyazi.md 7). */
export const turkceAltyaziCredit = "Turkish subtitles from TurkceAltyazi.org, fetched by this device when you play.";

/** When a version fails for a reason of its own (no picture here, a dead
 * swarm, the wrong file), the core moves on or offers the next one
 * (`PlaybackSnapshot.trying`, `offer`). */
export const nextVersion = {
  trying: (label: string) => `Trying the next version: ${label}`,
  play: (label: string) => `Play ${label}`,
  /** The failure card's headline: one version failed when the core offers
   * another (the card says Play {label}), else every source did. */
  failed: (offered: boolean) => (offered ? "This version could not be played" : "None of the sources could be played"),
  /** The card when the playback view could not open at all (the error goes under it). */
  notOpened: "This version could not be opened",
  /** The player's and Versions' card when the title's details could not be read. */
  titleNotOpened: "This title could not be opened",
  tryingAgain: "Trying again",
  /** A version picked by hand, waiting for Versions to list it (as Android). */
  finding: "Finding this version",
};

/** The busy line while a play starts over: a move on names the version it
 * tries first (as Android does), then Try again's own words. */
export function restartLine(s: { phase: string; trying: { label: string } | null } | null, again: boolean): string | null {
  if (s?.phase === "resolving" && s.trying) return nextVersion.trying(s.trying.label);
  return again ? nextVersion.tryingAgain : null;
}

export const tokenInLink = {
  warn: "This link carries a sign-in: a token or a key. Whoever has it uses that account. Install it only if it is yours.",
  confirm: "It is mine, install",
};

export const findAddons = {
  eyebrow: "Addons",
  title: "Find addons",
  lede: "Most of these add subtitles and catalogs. Sources usually come as a link from the addon's own website.",
  community: "Community addons are made by other people. They see what you open in them, and their catalogs show on Home on all your devices.",
  configureHelp: "Set it up on its website, copy the link it gives you, then paste it here.",
  configureTv: "Set it up on its website, then add the link in Marquee on your phone or computer. With sync on, it follows you here.",
  pasteLink: "Paste a link",
  listLabel: "List",
  official: "Official",
  communityList: "Community",
  showLabel: "Show",
  all: "All",
  sources: "Sources",
  subtitles: "Subtitles",
  catalogs: "Catalogs",
  loading: "Asking Cinemeta",
  failed: "Cinemeta did not answer.",
  tryAgain: "Try again",
  refresh: "Refresh",
  empty: "Nothing to show from Cinemeta here.",
  filterEmpty: "No {kind} addons in this list.",
  kindSources: "source",
  kindSubtitles: "subtitle",
  kindCatalogs: "catalog",
  installing: "Installing",
  turningOn: "Turning on",
  installed: "Installed",
  off: "Off",
  setUp: "Set up",
  setUpOff: "Set up, off",
  install: "Install",
  configure: "Configure",
  turnOn: "Turn on",
  fromHost: "from {host}",
} as const;

/** Devices and sync copy (plans/m7-sync.md). No protocol words: a device
 * list, not a roster; "the 12 characters to compare", not a fingerprint. */
export const syncWords = {
  title: "Devices and sync",
  lede: "Sync keeps your library, progress, addons and settings the same on all your devices. The sync server stores them sealed and cannot read them.",
  offLine: "Keep your library, progress and addons the same on every device.",
  tabOnly: "A browser has no safe place for this device's sync key, so it lasts until this tab closes or reloads. The apps keep it in the device's secure storage.",
  pairTitle: "Pair this device",
  pairText: "Show a code here and approve it on a device that already syncs.",
  startTitle: "Start a sync account",
  startText: "For your first device. Other devices join it with a code, so nothing is typed on them.",
  recoverTitle: "Use a recovery kit",
  recoverText: "Join your account again when you have none of your devices.",
  recoverLede: "Joins your account as a new device. The kit is the 32 letters and digits you saved when you made it.",
  server: "Sync server",
  serverHint: "https://sync.example.org",
  device: "This device",
  deviceHint: "Your other devices list it by this name.",
  invite: "Invite code",
  inviteHint: "Only if your sync server asks for one",
  kit: "Recovery kit",
  kitHint: "XXXX-XXXX-XXXX-XXXX-XXXX-XXXX-XXXX-XXXX",
  showKit: "Show the kit",
  hideKit: "Hide the kit",
  enterKit: "Enter the recovery kit.",
  badAddress: "Enter the address without a user name or password.",
  startButton: "Start the account",
  starting: "Starting the account",
  recoverButton: "Join with the kit",
  recovering: "Joining",
  wrongKit: "The sync server does not know this kit. Check the letters and try again.",
  alreadyJoined: "This device already syncs. To join another account, stop syncing here first.",
  plainLink: "Sync is off on this page: its link to Marquee's core is not encrypted. Open Marquee on the computer that runs the core, or use the app.",
  syncNow: "Sync now",
  syncing: "Syncing",
  notSynced: "Not synced yet",
  compareLabel: "Compare on each device",
  compareHelp: "Once they have synced, all your devices show these same 12 characters. If one shows different ones, the sync server may be hiding a device change.",
  devices: "Devices",
  loadingDevices: "Asking the sync server for your devices",
  thisDevice: "This device",
  unnamed: "Unnamed device",
  unknownDevice: "Unknown device",
  unknownLine: "Not in your device list yet",
  kitName: "Recovery kit",
  remove: "Remove",
  reportLost: "Report lost",
  cancel: "Cancel",
  retry: "Try again",
  dismiss: "Dismiss",
  addTitle: "Add a device",
  addText: "Approve a new device with the code it shows.",
  kitTitle: "Make a recovery kit",
  kitText: "Get your account back if you lose all your devices.",
  noKit: "This account has no recovery kit. If you lose all your devices, your synced data cannot be read again. Make one and keep it somewhere private.",
  noKitTv: "This account has no recovery kit. If you lose all your devices, your synced data cannot be read again. Make one on your phone or computer.",
  afterRecover: "You joined with a recovery kit. Make a new one now, and remove the device you lost.",
  afterRecoverTv: "You joined with a recovery kit. Make a new one on your phone or computer, and remove the device you lost.",
  rotationPending: "A device change is still finishing. Edits wait for it.",
  tvOnly: "On your phone or computer",
  tvAdd: "Add devices from your phone or computer: a TV has no camera to scan the code.",
  tvKit: "Make recovery kits on your phone or computer: on a TV, anyone in the room can read one.",
  leaveTitle: "Stop syncing on this device",
  leaveText: "Your library stays here. Edits not sent yet go out first.",
  leaveButton: "Stop syncing",
  leaveAnyway: "Stop anyway",
  syncAgain: "Sync again",
  left: "This device no longer syncs. Your library stays here.",
  // A different device list on the server
  useServer: "Compare device lists",
  forkTitle: "Compare device lists",
  forkLede: "The sync server lists different devices than this device knows. That happens when the server was restored from a backup, or when someone changed it. Check its list, tick anything you do not recognise, then keep syncing with it.",
  forkLoading: "Asking the sync server for its list",
  forkServer: "On the sync server's list",
  forkServerHelp: "Ticked devices are removed when you continue. Those marked as going are removed anyway: this device had removed them, or they are recovery kits only the server knows.",
  forkGoes: "Removed anyway",
  goesWithTicked: "Goes with {name}",
  forkMine: "Only this device knew these",
  forkMineHelp: "They are not on the server's list, so they are gone once you continue. Make a new recovery kit afterwards if one of them was yours.",
  forkSame: "Both lists name the same devices.",
  forkGo: "Continue with the server's list",
  forkGoing: "Continuing",
  forkOffList: "The sync server's list no longer includes this device: it was removed there. Pair this device again from one that still syncs.",
  forkChanged: "The sync server's list changed while you looked. Here it is again; check it before you continue.",
  forkTokenLost: "This device cannot read the sync server's list: the server no longer knows it. Sync from another of your devices, or pair this one again.",
  // Add a device
  addLede: "On the new device, open Settings, then Devices and sync, then Pair this device. It shows a code.",
  addWarning: "Approve only a code on a screen in front of you. Whoever controls that screen gets your account.",
  scan: "Scan the code",
  scanning: "Point the camera at the code on the other device.",
  noCamera: "The camera is not available. Paste the code instead.",
  pasteLabel: "Or paste the code",
  pasteOnly: "Paste the code",
  noScan: "This browser cannot scan codes. On the new device, choose Copy the code, then paste it here.",
  pasteHint: "marquee:pair?...",
  reviewAsk: "Approve a new device?",
  reviewText: "Approve it only if the new device is in front of you and shows the code you scanned. Whoever confirms the six digits next joins your account.",
  approve: "Approve",
  approving: "Approving",
  approvedStep: "The new device now shows six digits. They must be these:",
  approvedHelp: "If they match, confirm on the new device. If they differ, say so there, and press They differ here.",
  differ: "They differ",
  differEnded: "The approval ended and your sync key changed: whoever answered cannot join with it, and a device that already did is removed, with any device it added.",
  done: "Done",
  // The joining device's side
  copyCode: "Copy the code",
  copied: "Copied",
  copyHelp: "No camera? Copy the code, then paste it on the other device under Add a device.",
  // Recovery kit
  kitLede: "A recovery kit gets your account back when you have none of your devices. Anyone who has it can join your account, so keep it somewhere private, like a password manager.",
  kitFirst: "Your account is ready. Make its recovery kit now: without one, losing this device means losing what it syncs.",
  later: "Later",
  kitOnce: "Marquee shows the kit once and keeps no copy.",
  makeKit: "Make the kit",
  makingKit: "Making the kit",
  kitShown: "Your recovery kit",
  kitWarning: "Save it now. It is not shown again, and anyone who has it can join your account.",
  kitClipboard: "The kit is on your clipboard. Paste it into your password manager now: Marquee empties the clipboard when you leave this page.",
  copyKit: "Copy",
  savedKit: "I saved it",
  // Remove and report lost
  removeKitAsk: "Remove this recovery kit? It stops working.",
  lostLede: "{name} is removed from your account and cannot read anything you change from now on.",
  lostAdded: "Devices added through {name}",
  lostAddedHelp: "Whoever has {name} may have these too, so they are removed as well. Untick the ones you still have.",
  lostChecking: "Checking what {name} added",
  removed: "Removed",
  listStale: "Done, but the device list did not refresh. Try again to see it.",
  // Remember this browser
  rememberTitle: "Remember this browser",
  rememberText: "Keeps this device in your account after a reload, locked with a passkey. Opening it again, removing a device, reporting one lost and making a kit each ask you to confirm with your face, finger or device PIN.",
  rememberButton: "Remember with a passkey",
  remembering: "Waiting for the passkey",
  remembered: "Remembered in this browser, locked with a passkey.",
  forgetBrowser: "Forget in this browser",
  noPasskey: "This browser cannot lock the sync key with a passkey, so it stays in this tab only and a reload forgets it.",
  noPrf: "This passkey cannot lock the sync key, so this browser keeps it in this tab only. A passkey from another manager may work.",
  forgetFailed: "This browser could not forget the sync account: it still holds it, locked with your passkey.",
  leftKept: "This device no longer syncs, but this browser could not forget the account: it still holds it, locked with your passkey, and reloading this page would offer to open it again. Forget it here to finish.",
  vaultUnreadable: "This browser could not check whether it remembers a sync account, so nothing new starts here. Check again.",
  checkAgain: "Check again",
  checkingNow: "Checking",
  continueText: "It failed again. You can sync in this tab without this browser's storage: nothing is read from it or stored in it, and a reload checks again.",
  joining: "This device is already joining a sync account. Wait for that to finish, then try again.",
  wrongRecord: "This device does not hold this sync account any more, so there is nothing to stop syncing here. Reload the page; if it then offers to unlock the account, forget it in this browser.",
  continueWithout: "Continue without remembering",
  memoryOnly: "This browser's storage could not be read, so nothing is remembered here: the sync key lasts until this tab closes or reloads.",
  saveFailed: "This browser could not store the newest sync key. The copy it keeps still opens this account, and it tries again with the next change.",
  saveAgain: "Save again",
  storageFailed: "This browser could not store the sync key, so it does not remember this account. It lasts until this tab closes.",
  checking: "Checking whether this browser remembers a sync account",
  leaveHere: "Stop syncing here only",
  leaveHereText: "This device forgets the account without telling the sync server, which lists it until another device removes it. Your library stays here, and you can pair this device again.",
  leftHere: "This device no longer syncs. The sync server still lists it until another device removes it; you can pair it again.",
  notRemembered: "Not remembered: the sync key lasts until this tab closes or reloads.",
  lockedFirst: "This browser remembers a sync account. Unlock it or forget it on Devices and sync before joining another.",
  forkNone: "This device and the sync server list the same devices. There is nothing to compare.",
  locked: "This browser remembers your sync account, locked with a passkey.",
  unlock: "Unlock with your passkey",
  unlocking: "Waiting for the passkey",
  passkeyAsk: "This asks for your passkey first.",
} as const;

/** Fills `{name}` in a sync line. */
export const named = (line: string, name: string) => line.replaceAll("{name}", name);

/** "1 device" or "3 devices". */
export const count = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

/** The confirm line for removing another device. */
export const removeDeviceAsk = (name: string) => `Remove ${name}? It stops syncing and cannot read anything you change from now on. Devices it added stay.`;

/** The button that confirms a lost report: it names how many devices go. */
export const lostButton = (devices: number) => `Remove ${count(devices, "device", "devices")}`;

/** The kits a lost report removes, always. */
export const lostKits = (kits: number) =>
  `${kits === 1 ? "1 recovery kit made on these devices stops" : `${kits} recovery kits made on these devices stop`} working. Make a new one afterwards.`;

/** The button that continues with the server's list, naming what the user ticked. */
export function forkButton(devices: number, kits = 0): string {
  const parts = [devices ? count(devices, "device", "devices") : "", kits ? count(kits, "recovery kit", "recovery kits") : ""].filter(Boolean);
  return parts.length ? `Continue and remove ${parts.join(" and ")}` : syncWords.forkGo;
}

/** On a ticked row of the server's list: the kits that go with it. */
export const goesWith = (kits: number) => (kits === 1 ? "Its recovery kit goes with it." : `Its ${kits} recovery kits go with it.`);

/** A member's second line: when it was added, and through which device. */
export function addedLine(kit: boolean, addedMs: number, by: string | null): string {
  const when = `${new Intl.DateTimeFormat("en", { dateStyle: "medium" }).format(addedMs)}, ${new Intl.DateTimeFormat("en", { timeStyle: "short" }).format(addedMs)}`;
  if (kit) return by ? `Made ${when} on ${by}` : `Made ${when}`;
  return by ? `Added ${when} through ${by}` : `Added ${when}, the first device`;
}

/** A sync failure in words, by the core's `code` first (as Apple's
 * `SyncProblem` and Android's `SyncCopy` word them), then the app's own
 * (`passkey`, `plainLink`, `alreadyJoined`). The core's own sentences are
 * kept; anything that may name the server's address is replaced. */
export function syncProblem(code: string | undefined, message: string): string {
  switch (code) {
    case "dropped":
      return "This device was removed from your sync account. Your library stays here as local data.";
    case "fork":
      return "The sync server lists different devices than this device knows. Compare the lists to keep syncing.";
    case "wrongRelay":
      return "This is not the sync server this device joined. Pair this device again.";
    case "relayBehind":
      return "The sync server lost device changes it cannot get back. Pair this device again from one that still syncs.";
    case "notJoined":
      return "This device has not joined a sync account yet.";
    case "unsent":
      return "Some changes have not been sent yet. Sync again first, or stop anyway and they stay only here.";
    case "insecure":
      return "The sync server address must start with https.";
    case "passkey":
      return "The passkey did not confirm it, so nothing changed.";
    case "noPrf":
      return syncWords.noPrf;
    case "forget":
      return syncWords.forgetFailed;
    case "plainLink":
      return syncWords.plainLink;
    case "alreadyJoined":
      return syncWords.alreadyJoined;
    case "rosterChanged":
      return syncWords.forkChanged;
    case "wrongRecord":
      return syncWords.wrongRecord;
    case "joining":
      return syncWords.joining;
  }
  if (message.startsWith("roster: ")) return pairingProblem(`pairing: ${message.slice("roster: ".length)}`);
  if (message.startsWith("pairing: ") || message.startsWith("invalid url") || message.startsWith("blocked private address") || message.startsWith("the relay must use https"))
    return pairingProblem(message);
  return "The sync server did not answer. Your changes wait here and go out on the next sync.";
}

/** What did not happen, per action, when the sync server did not answer. */
const notDone = {
  approve: "Nothing was approved.",
  load: "The device list did not load.",
  kit: "No kit was made.",
  remove: "Nothing was removed.",
  lost: "Nothing was removed.",
  leave: "This device still syncs.",
  serverList: "Nothing changed.",
  compare: "The lists could not be compared.",
  unlock: "Sync is still locked.",
  differ: "The approval is still open.",
} as const;

export type SyncStep = "start" | "recover" | "remember" | keyof typeof notDone;

/** A failure of one action in words: by code, then the step's own statuses
 * (an invite on start, an unknown kit on recover, a used code on approve),
 * then what did not happen. Never the raw text: it can name the server. */
export function stepProblem(code: string | undefined, message: string, step: SyncStep): string {
  if (code) return syncProblem(code, message);
  // Remembering fails in this browser's storage, not on the sync server.
  if (step === "remember") return syncWords.storageFailed;
  if (step === "start" && message === "http status 403") return "This sync server needs an invite code, or the one entered is wrong.";
  if (step === "recover" && message === "http status 404") return syncWords.wrongKit;
  if (step === "start" || step === "recover") return pairingProblem(message);
  if (step === "compare" && message === "http status 401") return syncWords.forkTokenLost;
  if (step === "approve" && (message === "http status 404" || message === "http status 410")) return "This code expired or was used already. Ask the new device for a new one.";
  if (message.startsWith("pairing: ") || message.startsWith("roster: ")) return syncProblem(undefined, message);
  return `${notDone[step]} The sync server did not answer. Try again in a moment.`;
}

type Facts = Pick<SyncReport, "clockGapMs" | "waiting" | "parked" | "oldEpochWrites" | "rotationWaitMs" | "blocked" | "refused" | "unreadable" | "newer" | "again">;

/** How a sync round ended: everything went, some changes still wait, or some failed. */
export type Outcome = "complete" | "pending" | "failed";
export function outcomeOf(r: Facts): Outcome {
  if (r.refused > 0 || r.unreadable > 0) return "failed";
  if (r.waiting > 0 || r.parked > 0 || r.newer > 0 || r.blocked !== null || r.rotationWaitMs > 0 || r.again) return "pending";
  return "complete";
}

/** The state line after a round, with its time ("2:05 PM"). */
export function outcomeLine(outcome: Outcome, time: string): string {
  if (outcome === "failed") return `Synced at ${time}. Some changes did not sync.`;
  if (outcome === "pending") return `Synced at ${time}. Some changes still wait.`;
  return `Synced at ${time}`;
}

/** What a sync round's facts mean for the user (plans/core-protocol.md, `syncNow`). */
export function reportNotes(r: Facts): string[] {
  const notes: string[] = [];
  if (r.refused > 0)
    notes.push(`${r.refused === 1 ? "1 edit" : `${r.refused} edits`} from this device could not be sent: the sync server does not take ${r.refused === 1 ? "it" : "them"}, so ${r.refused === 1 ? "it stays" : "they stay"} on this device only.`);
  if (r.unreadable > 0) notes.push(`${r.unreadable === 1 ? "1 change from another device" : `${r.unreadable} changes from other devices`} could not be read and ${r.unreadable === 1 ? "was" : "were"} skipped.`);
  if (r.newer > 0) notes.push(`${r.newer === 1 ? "1 change comes" : `${r.newer} changes come`} from a newer version of Marquee. Update Marquee here to see ${r.newer === 1 ? "it" : "them"}.`);
  const gap = Math.abs(r.clockGapMs);
  if (gap > 600_000) {
    const minutes = Math.round(gap / 60_000);
    notes.push(`The sync server's clock and this device's disagree by ${count(minutes, "minute", "minutes")}, so edits sync in the order they are sent.`);
  }
  if (r.waiting > 0) notes.push(`${r.waiting === 1 ? "1 edit waits" : `${r.waiting} edits wait`} for the sync server's clock to catch up. Nothing is lost.`);
  if (r.parked > 0) notes.push(`${r.parked === 1 ? "1 change from another device waits" : `${r.parked} changes from other devices wait`} until this device catches up with a device change.`);
  if (r.oldEpochWrites > 0) notes.push("The sync server may be hiding a device change. Compare the 12 characters below on each device.");
  if (r.rotationWaitMs > 0) {
    const minutes = Math.max(1, Math.ceil(r.rotationWaitMs / 60_000));
    notes.push(`Edits wait ${count(minutes, "minute", "minutes")} more: the sync server was restored, and other devices get a chance to catch up first.`);
  }
  if (r.blocked === "rotationPending") notes.push("Edits wait until the account finishes a device change.");
  if (r.blocked === "notActive") notes.push("The sync server does not take edits from this device yet, so they were not sent.");
  if (r.blocked === "relayDisagrees") notes.push("The sync server disagrees about your device list, so edits were not sent. Compare the 12 characters below on each device.");
  if (r.again) notes.push("The last sync stopped before it finished. It goes on with the next one.");
  return notes;
}

/** Failures since this device joined, kept until the user dismisses them. */
export function failedSince(refused: number, unreadable: number, newer = 0): string | null {
  const parts = [
    refused ? `${count(refused, "edit", "edits")} from this device could not be sent and stay${refused === 1 ? "s" : ""} here only` : "",
    unreadable ? `${count(unreadable, "change", "changes")} from other devices could not be read` : "",
    newer ? `${count(newer, "change", "changes")} from a newer version of Marquee wait${newer === 1 ? "s" : ""} for an update here` : "",
  ].filter(Boolean);
  return parts.length ? `Since this device joined, ${parts.join(", and ")}.` : null;
}

/** The Settings row under Devices and sync: what is wrong or waits first, then the time. */
export function syncSummary(s: { locked: boolean; problem: string | null; joined: boolean; refused: number; unreadable: number; newer?: number; report: Facts | null; time: string | null }): string {
  // A problem first: a storage that could not be read is locked too, and "locked with a passkey" would be wrong.
  if (s.problem) return s.problem;
  if (s.locked) return syncWords.locked;
  if (!s.joined) return syncWords.offLine;
  const failed = failedSince(s.refused, s.unreadable, s.newer ?? 0);
  if (failed) return failed;
  const note = s.report && outcomeOf(s.report) !== "complete" ? reportNotes(s.report)[0] : undefined;
  if (note) return note;
  return s.time ? `On. Synced at ${s.time}.` : "On.";
}

// Smart stream choice (plans/m13-stream-choice.md 3.7, 3.9, 3.11, 4.4, 4.5).
// The words live in apps/shared/stream-choice-words.json so Android and Apple
// say the same; its "about" gives the rules every app follows.

export const choiceWords = choice;

type Fields = Record<string, string | number | boolean | null | undefined>;

/** Fills `{name}` fields; null when one is unknown, so the part is left out. */
function part(line: string, fields: Fields): string | null {
  let known = true;
  const text = line.replace(/\{(\w+)\}/g, (_, name: string) => {
    const v = fields[name];
    if (v === null || v === undefined) known = false;
    return String(v ?? "");
  });
  return known ? text : null;
}

const capital = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);

/** "1 seeder", "1,204 seeders". */
export const counted = (what: keyof typeof choice.plurals, n: number) =>
  (choice.plurals[what][n === 1 ? 0 : 1] as string).replace("{n}", n.toLocaleString("en-US"));

const numbers = (l: ReasonLine): Fields => {
  const f: Fields = { ...l };
  if ("seeders" in l) f.seeders = l.seeders === null ? null : counted("seeders", l.seeders);
  if ("sizeBytes" in l) f.size = l.sizeBytes === null ? null : bytes(l.sizeBytes);
  if (l.kind === "large") f.limit = bytes(l.limitBytes);
  if (l.kind === "mayPause") {
    f.need = Math.round(l.needMbps);
    f.have = Math.round(l.haveMbps);
    f.speedSource = l.samples > 0 ? choice.speedSource.measured.replace("{playbacks}", counted("playbacks", l.samples)) : choice.speedSource.estimated;
  }
  return f;
};

/** A version's line: the tone, the default text, and the details shown on
 * focus (TV), hover (desktop) or tap (phone). A large file says "Larger
 * than the size you chose" when the user set the limit. */
export function lineWords(l: ReasonLine): { tone: Exclude<Tone, "idle">; text: string; details: string } {
  const w = choice.lines[l.kind];
  const text = l.kind === "large" && l.chosen && "textChosen" in w ? w.textChosen : w.text;
  const f = numbers(l);
  const details = w.details.map((d) => part(d, f)).filter((d) => d !== null);
  return { tone: w.tone as Exclude<Tone, "idle">, text, details: capital(details.join(", ")) };
}

/** Why no version is visible, with its one action (3.11). */
export function blockedWords(reason: BlockedReason, count: number) {
  const w = choice.blocked[reason];
  return { text: w.text.replace("{versions}", counted("versions", count)), action: w.action };
}

/** What the device line says: the screen, the connection and the disk. */
export interface DeviceConditions {
  screenClass: number;
  hdr: boolean;
  network: Network;
  speed: Speed;
  freeBytes: number | null;
}

/** The speed that decides here: HTTP with a streaming service account or
 * without the engine, else the torrent engine's. */
export const speedFor = (c: { http: Speed; p2p: Speed; engine: boolean }, account: boolean) => (account || !c.engine ? c.http : c.p2p);

const c = choice.conditions;

export const deviceName = (f: Form) => c.devices[f];

const screenWord = (cls: number) => c.screens[String(cls) as keyof typeof c.screens] ?? `${cls}p`;

/** "4K HDR, about 42 Mbit/s on Wi-Fi, 61 GB free". */
export function conditionsLine(d: DeviceConditions): string {
  const screen = screenWord(d.screenClass);
  const network = c.networks[d.network];
  const speed = d.speed.samples > 0 ? part(c.speed, { mbps: Math.round(d.speed.mbps), network }) : part(c.speedUnknown, { network });
  const free = d.freeBytes === null ? null : part(c.free, { free: bytes(d.freeBytes) });
  return [d.hdr ? part(c.hdr, { screen }) : screen, speed, free].filter((x) => x !== null).join(", ");
}

/** The Versions header from the conditions the ranking read. */
export const conditionsHeader = (f: Form, v: VersionsConditions) =>
  capital(
    c.header
      .replace("{device}", deviceName(f))
      .replace("{conditions}", conditionsLine({ screenClass: v.screenClass, hdr: v.hdr, network: v.network, speed: speedFor(v, v.account), freeBytes: v.freeBytes })),
  );

/** The one-time note on Versions (3.9), with the preferences now in effect:
 * "On this TV you often picked bigger files, so Marquee now picks files up
 * to 10 GB." Null when the change names no number (no limit, the screen's
 * resolution). */
export function learnedNote(choice_: LearnedChoice, up: boolean, f: Form, prefs: Pick<StreamPrefs, "sizeLimitBytes" | "maxHeight">): string | null {
  const limit = prefs.sizeLimitBytes === null ? null : bytes(prefs.sizeLimitBytes);
  const height = prefs.maxHeight === null ? null : screenWord(prefs.maxHeight);
  return part(choice.learned[choice_][up ? "up" : "down"], { device: deviceName(f), limit, height });
}

// Largest screen first (an object with number keys would list them smallest first).
const classes = [
  [2160, "uhd"],
  [1080, "fhd"],
  [720, "hd"],
  [480, "sd"],
] as const;
const choices: LearnedChoice[] = ["size", "height", "ready", "hdr"];

/** What the picks taught, for Settings: each choice at 3 or more either
 * way, by screen class, unless undone or forgotten (a class stamped with an
 * older epoch reads as zero). */
export function learnedRows(p: Pick<Prefs, "learned" | "learnedEpoch">): { screenClass: number; choice: LearnedChoice; up: boolean; text: string }[] {
  return classes.flatMap(([cls, key]) => {
    const l = p.learned[key];
    if (l.epoch < p.learnedEpoch) return [];
    return choices
      .filter((ch) => Math.abs(l[ch]) >= 3 && !l.muted.includes(ch))
      .map((ch) => {
        const up = l[ch] > 0;
        const screens = choice.learned.screens[String(cls) as keyof typeof choice.learned.screens];
        return { screenClass: cls, choice: ch, up, text: choice.learned.settingsLines[ch][up ? "up" : "down"].replace("{screens}", screens) };
      });
  });
}

/** Anything learned that Forget would clear. */
export const anyLearned = (p: Pick<Prefs, "learned" | "learnedEpoch">) =>
  classes.some(([, key]) => p.learned[key].epoch >= p.learnedEpoch && choices.some((ch) => p.learned[key][ch] !== 0));

/** "Up to 5 GB, shared by at least 50 people": the Settings row's line. */
export function choiceSummary(p: Pick<StreamPrefs, "sizeLimitBytes" | "minSeeders">): string {
  const s = choice.settings;
  const sizeLabel = s.size.choices.find((x) => x.value === p.sizeLimitBytes)?.label ?? s.size.custom.replace("{size}", bytes(p.sizeLimitBytes ?? 0));
  const people = s.seeders.choices.find((x) => x.value === p.minSeeders)?.label ?? s.seeders.custom.replace("{n}", p.minSeeders.toLocaleString("en-US"));
  return s.summary.replace("{size}", sizeLabel).replace("{people}", people);
}

/** `displayHeight` to the core's screen class (3.3): an iPhone (1179 lines)
 * and a 1440 phone are 1080. */
export const screenClass = (lines: number) => (lines >= 2000 ? 2160 : lines >= 1000 ? 1080 : lines >= 700 ? 720 : 480);

/** No addon found a stream: one wording on the title page, Versions and the player. */
export const noVersionWords = {
  title: "No playable version found",
  detail: "None of your addons found a stream for this. A source addon finds them; add one in Addons.",
};

/** Addons still answering past their time: "Torrentio is slow", "Torrentio, Comet are slow". */
export const slowWords = (names: string[]) => `${names.join(", ")} ${names.length > 1 ? "are" : "is"} slow, still waiting`;
