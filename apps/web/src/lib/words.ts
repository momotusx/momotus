/** Plain-language copy for the core's typed facts. The core sends enums and
 * codes; every UI turns them into the same words. */

import type { Group } from "../core/generated/Group";
import type { HiddenSummary } from "../core/generated/HiddenSummary";
import type { HideReason } from "../core/generated/HideReason";
import type { Reason } from "../core/generated/Reason";
import type { SyncReport } from "../core/generated/SyncReport";
import type { Tile } from "../core/generated/Tile";
import type { VersionStatus } from "../core/generated/VersionStatus";

export function reasonText(r: Reason, status: VersionStatus): string {
  switch (r) {
    case "readyNow":
      return status.kind === "ready" && status.service ? `Ready now, cached on ${status.service}` : "Ready now";
    case "playsDirectly":
      return "Plays directly on this device";
    case "matchesScreen":
      return "Matches your screen";
    case "sameReleaseAsLast":
      return "Same release as the last episode";
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
  forkGoes: "Goes",
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
  differEnded: "The approval ended and your sync key changed: whoever answered cannot join with it, and a device that already did is removed.",
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
  rememberLost: "This browser could not keep the sync key, so it no longer remembers this account. Remember it again, or it lasts until this tab closes.",
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
export const forkButton = (devices: number, kits = 0) =>
  devices ? `Continue and remove ${count(devices, "device", "devices")}${kits ? ` and ${kits === 1 ? "1 recovery kit" : `${kits} recovery kits`}` : ""}` : syncWords.forkGo;

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
    case "plainLink":
      return syncWords.plainLink;
    case "alreadyJoined":
      return syncWords.alreadyJoined;
    case "rosterChanged":
      return syncWords.forkChanged;
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
} as const;

export type SyncStep = "start" | "recover" | keyof typeof notDone;

/** A failure of one action in words: by code, then the step's own statuses
 * (an invite on start, an unknown kit on recover, a used code on approve),
 * then what did not happen. Never the raw text: it can name the server. */
export function stepProblem(code: string | undefined, message: string, step: SyncStep): string {
  if (code) return syncProblem(code, message);
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
  if (s.locked) return syncWords.locked;
  if (s.problem) return s.problem;
  if (!s.joined) return syncWords.offLine;
  const failed = failedSince(s.refused, s.unreadable, s.newer ?? 0);
  if (failed) return failed;
  const note = s.report && outcomeOf(s.report) !== "complete" ? reportNotes(s.report)[0] : undefined;
  if (note) return note;
  return s.time ? `On. Synced at ${s.time}.` : "On.";
}
