import { A, useNavigate, useParams } from "@solidjs/router";
import Check from "lucide-solid/icons/check";
import ChevronRight from "lucide-solid/icons/chevron-right";
import Download from "lucide-solid/icons/download";
import KeyRound from "lucide-solid/icons/key-round";
import PackageSearch from "lucide-solid/icons/package-search";
import Plug from "lucide-solid/icons/plug";
import Puzzle from "lucide-solid/icons/puzzle";
import MonitorSmartphone from "lucide-solid/icons/monitor-smartphone";
import Radio from "lucide-solid/icons/radio";
import Rows3 from "lucide-solid/icons/rows-3";
import SlidersHorizontal from "lucide-solid/icons/sliders-horizontal";
import Sparkles from "lucide-solid/icons/sparkles";
import Trash from "lucide-solid/icons/trash";
import ArrowDownToLine from "lucide-solid/icons/arrow-down-to-line";
import Captions from "lucide-solid/icons/captions";
import CirclePlay from "lucide-solid/icons/circle-play";
import Info from "lucide-solid/icons/info";
import LogOut from "lucide-solid/icons/log-out";
import Link2 from "lucide-solid/icons/link-2";
import LogIn from "lucide-solid/icons/log-in";
import Monitor from "lucide-solid/icons/monitor";
import User from "lucide-solid/icons/user";
import Wrench from "lucide-solid/icons/wrench";
import { createEffect, createResource, createSignal, For, type JSX, on, onMount, Show } from "solid-js";
import { relayProviders, signInServer } from "../core/sign-in";
import { mayDeleteAccount } from "../lib/sync";
import type { PluginsAnswer } from "../core/generated/PluginsAnswer";
import { pluginWords, pluginsSummary } from "../lib/plugins";
import { deviceLanguage, homeView, liveView, op, prefsView, reasonOf } from "../core/client";
import { Backdrop, useTrimmedLogo } from "../components/art";
import { CONTINUE_ROW } from "../components/row";
import { focusEl, reducedMotion } from "../focus/dom";
import type { AddonsSnapshot } from "../core/generated/AddonsSnapshot";
import type { DownloadsSnapshot } from "../core/generated/DownloadsSnapshot";
import { downloadAct, downloadsHere } from "../components/download";
import { deleteAll, downloadWords, shownDownloads } from "../lib/download";
import { focusOn, StorageLine } from "./downloads";
import type { LanguageList } from "../core/generated/LanguageList";
import type { Prefs } from "../core/generated/Prefs";
import type { SubtitleLayout } from "../core/generated/SubtitleLayout";
import type { SubtitleSize } from "../core/generated/SubtitleSize";
import { chooseForm, type Form, form, pinned } from "../form";
import { headerOnTvNow, previewsOn, setPreviewsOn } from "../lib/trailers";
import { defaultsWords } from "../lib/defaults";
import { addonsSummary, choiceSummary, choiceWords, findAddons, languageName, syncSummary, syncWords, tmdbCredit } from "../lib/words";
import * as sync from "../core/sync";
import { traktLink, traktLockedHere, traktStatus, traktStorage } from "../core/trakt";
import { rowLine, traktWords } from "../lib/trakt";
import "./browse.css";
import { markSrc } from "../components/marks";
import "./settings.css";
import "./onboarding.css";
import { LanguageOverlay, LanguageSearch, NoneFlag, SPOKEN, SpokenFlag } from "./language-picker";
import { Flag } from "../player/flags";
import { onFirstLine, settingsHint, summary, tap, tapAction, tapWords } from "../lib/language-picker";
import UserPen from "lucide-solid/icons/user-pen";
import Users from "lucide-solid/icons/users";
import { AvatarArt } from "../components/avatar";
import { incognito, profiles } from "../core/profiles";
import { current } from "../lib/profiles";
import { debridAccounts } from "../core/debrid";
import { inUse } from "../lib/debrid";
import { accountWords, connectGoogle, connectText, connectTitle } from "../lib/account";
import { sectionId } from "../lib/settings-sections";
import { incognitoWords, useLeave } from "../components/incognito";

const clockTime = (ms: number) => new Date(ms).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });

/** The ISO 639-2 codes earlier pickers stored for subtitles; the core reads both. */
const LEGACY: Record<string, string> = { eng: "en", tur: "tr", spa: "es", fre: "fr", ger: "de", ita: "it", por: "pt", jpn: "ja" };

// Every language's own name as the core names it, asked once per app run (and again after a failure).
const ownNames = new Map<boolean, Promise<Record<string, string>>>();
const namesFor = (audio: boolean) => {
  let names = ownNames.get(audio);
  if (!names) {
    names = op<LanguageList>("languagePicker", { device: deviceLanguage(), audio }).then((reply) =>
      Object.fromEntries(reply.results.map((l) => [l.code, l.native || l.name])),
    );
    names.catch(() => ownNames.delete(audio));
    ownNames.set(audio, names);
  }
  return names;
};

/** A language picker's row in Settings (subtitles; audio with `audio`): the first chosen language's flag,
 * its value ("Türkçe first, 2 more", or `none` with nothing chosen), and the search-first picker over the
 * page when it is pressed (plans/onboarding-language-picker.md). Subtitles: a visit's picks rank in tap
 * order before the older ones, a tap on an older pick makes it the default, a tap on the default (or on a
 * pick of this visit) removes it. Audio: a new pick goes last, a tap removes. Each tap is written at once;
 * `onChange` answers whether it was saved. Subtitles store the core's tag as is, audio the part before "-"
 * ("pt" for "pt-BR"). */
export function LanguagePicker(props: {
  label: string;
  help?: string;
  /** The value with nothing chosen. */
  none?: string;
  group: string;
  chosen: string[];
  audio?: boolean;
  onChange: (next: string[]) => Promise<boolean>;
}) {
  const [open, setOpen] = createSignal(false);
  const [names, setNames] = createSignal<Record<string, string>>({});
  // A visit's additions (the undo rule); a new visit starts empty.
  const [added, setAdded] = createSignal<ReadonlySet<string>>(new Set());
  const codeOf = (tag: string) => (props.audio ? (tag.split("-")[0] ?? tag) : tag);
  const norm = (v: string) => codeOf(LEGACY[v.toLowerCase()] ?? v);
  const saved = () => [...new Set(props.chosen.map(norm))];
  // The list as last tapped, ahead of the saved one, so two quick taps inside one save both count. It
  // gives way once the saved list matches it, or when a save fails (the row then shows what is saved and
  // the page says why).
  const [local, setLocal] = createSignal<string[] | null>(null);
  createEffect(on(saved, (s) => local()?.join() === s.join() && setLocal(null), { defer: true }));
  const chosen = () => local() ?? saved();
  onMount(() => void namesFor(!!props.audio).then(setNames, () => undefined));
  const nameOf = (code: string) => names()[code] ?? languageName(code);
  const value = () => summary(chosen().map(nameOf), props.none ?? "Any language");
  const write = (next: string[]) => {
    setLocal(next);
    void props.onChange(next).then((ok) => ok || setLocal(null));
  };
  const pick = (code: string | null) => {
    if (code === null) {
      if (chosen().length === 0) return;
      setAdded(new Set<string>());
      write([]);
      return;
    }
    const next = tap(chosen(), code, added(), !!props.audio);
    setAdded(next.added);
    write(next.list);
  };
  const isOn = (code: string | null) => (code === null ? chosen().length === 0 : chosen().includes(code));
  return (
    <>
      <button
        class="link-row lp-setting"
        data-f={`${props.group}:open`}
        aria-haspopup="dialog"
        onClick={() => {
          setAdded(new Set<string>());
          setOpen(true);
        }}
      >
        <Show when={chosen()[0]} fallback={<NoneFlag />}>
          {(c) => <Flag language={c()} />}
        </Show>
        <span>
          <b>{props.label}</b>
          <small>{value()}</small>
          <Show when={props.help}>
            <small>{props.help}</small>
          </Show>
        </span>
        <ChevronRight aria-hidden class="chev" />
      </button>
      <Show when={open()}>
        <LanguageOverlay title={props.label} done="Done" onClose={() => setOpen(false)}>
          <LanguageSearch
            group={props.group}
            audio={props.audio}
            multi
            chosen={chosen()}
            isOn={isOn}
            rank={(code) => chosen().indexOf(code) + 1}
            action={(code) => (code === null ? (isOn(null) ? "" : "Remove all") : tapWords[tapAction(chosen(), code, added(), !!props.audio)])}
            onPick={pick}
            focusField
            focusFirst
            hint={settingsHint(!!props.audio)}
          />
        </LanguageOverlay>
      </Show>
    </>
  );
}

const dual = choiceWords.player;

/** Settings' Second line (plans/dual-subtitles.md 5.5): its value, and over the page the language picker with Off and
 * Spoken language first; the first subtitle language's row reads "Subtitle language" and cannot be picked. A pick is
 * written at once and closes the picker. */
function SecondLine(props: { chosen: string | null; first: string | undefined; onPick: (v: string | null) => void }) {
  const [open, setOpen] = createSignal(false);
  const [names, setNames] = createSignal<Record<string, string>>({});
  onMount(() => void namesFor(false).then(setNames, () => undefined));
  const value = () => (props.chosen === null ? "Off" : props.chosen === SPOKEN ? dual.spokenLanguage : (names()[props.chosen] ?? languageName(props.chosen)));
  return (
    <>
      <button class="link-row lp-setting" data-f="pref-second:open" aria-haspopup="dialog" onClick={() => setOpen(true)}>
        <Show when={props.chosen} fallback={<NoneFlag />}>
          {(c) => (c() === SPOKEN ? <SpokenFlag /> : <Flag language={c()} />)}
        </Show>
        <span>
          <b>{dual.secondLine}</b>
          <small>{value()}</small>
          <small>A second subtitle in another language, with the first. Spoken language follows each film's audio.</small>
        </span>
        <ChevronRight aria-hidden class="chev" />
      </button>
      <Show when={open()}>
        <LanguageOverlay title={dual.secondLine} done="Done" onClose={() => setOpen(false)}>
          <LanguageSearch
            group="pref-second"
            none="Off"
            spoken={dual.spokenLanguage}
            chosen={props.chosen && props.chosen !== SPOKEN ? [props.chosen] : []}
            isOn={(code) => code === props.chosen}
            blocked={(code) => (onFirstLine(code, props.first) ? "Subtitle language" : undefined)}
            onPick={(code) => {
              if (code !== props.chosen) props.onPick(code);
              setOpen(false);
            }}
            focusField
            focusFirst
          />
        </LanguageOverlay>
      </Show>
    </>
  );
}

function ChoiceChip<T>(props: { group: string; item: { value: T; label: string }; on: boolean; multi?: boolean; autofocus?: boolean; onPick: (v: T) => void }) {
  return (
    <button
      class="chip"
      classList={{ on: props.on }}
      role={props.multi ? "checkbox" : "radio"}
      aria-checked={props.on}
      data-f={`${props.group}:${String(props.item.value)}`}
      data-autofocus={props.autofocus ? "" : undefined}
      onClick={() => props.onPick(props.item.value)}
    >
      <Show when={props.on}>
        <Check aria-hidden class="chip-check" />
      </Show>
      {props.item.label}
    </button>
  );
}

export function Choice<T>(props: {
  label: string;
  /** A line under the label (the Stream choice page's rows). */
  help?: string;
  group: string;
  items: { value: T; label: string }[];
  isOn: (v: T) => boolean;
  onPick: (v: T) => void;
  autofocus?: boolean;
  multi?: boolean;
}) {
  return (
    <div class="chips-row wrap-row" role={props.multi ? "group" : "radiogroup"} aria-label={props.label} data-fgroup={props.group}>
      <span class="kicker">
        {props.label}
        <Show when={props.help}>
          <small>{props.help}</small>
        </Show>
      </span>
      <div class="chips-track wrap">
        <For each={props.items}>
          {(item, i) => (
            <ChoiceChip
              group={props.group}
              item={item}
              on={props.isOn(item.value)}
              multi={props.multi}
              autofocus={props.autofocus && i() === 0}
              onPick={props.onPick}
            />
          )}
        </For>
      </div>
    </div>
  );
}

export function LinkRow(props: { href: string; icon: JSX.Element; title: string; text: string; fkey: string }) {
  return (
    <A class="link-row" href={props.href} data-f={props.fkey} data-autofocus={props.fkey === "settings:addons" ? "" : undefined}>
      {props.icon}
      <span>
        <b>{props.title}</b>
        <small>{props.text}</small>
      </span>
      <ChevronRight aria-hidden class="chev" />
    </A>
  );
}

/** An icon tile's fill: one of the blue-to-green steps (design/DECISION.md, Settings A1), never red or orange. */
type Tone = "blue" | "sky" | "sea" | "teal" | "mint" | "green" | "grey";

const Tile = (props: { tone: Tone; children: JSX.Element }) => (
  <span class={`stile ${props.tone}`} aria-hidden="true">
    {props.children}
  </span>
);

/** One row of a grouped list: its tile, its words, a chevron; the whole row is the link. */
function Row(props: { href: string; fkey: string; tone: Tone; icon: JSX.Element; title: string; text: string }) {
  return (
    <A class="srow" href={props.href} data-f={props.fkey}>
      <Tile tone={props.tone}>{props.icon}</Tile>
      <span class="stext">
        <b>{props.title}</b>
        <small>{props.text}</small>
      </span>
      <ChevronRight aria-hidden class="chev" />
    </A>
  );
}

/** The section menu's entries, with the line the phone's list shows under each. Profiles is the header. */
const SECTIONS: { id: string; label: string; tone: Tone; icon: () => JSX.Element; line: string }[] = [
  { id: "profiles", label: "Profiles", tone: "blue", icon: () => <User />, line: "" },
  { id: "sources", label: "Sources", tone: "green", icon: () => <Puzzle />, line: "Addons, plugins, debrid services, Trakt and import" },
  { id: "account", label: "Account", tone: "sky", icon: () => <MonitorSmartphone />, line: "Sign in and sync with your other devices" },
  { id: "home", label: "Home", tone: "mint", icon: () => <Rows3 />, line: "Which rows Home shows" },
  { id: "playback", label: "Playback", tone: "teal", icon: () => <CirclePlay />, line: "Stream choice and up next" },
  { id: "subtitles", label: "Subtitles and audio", tone: "sky", icon: () => <Captions />, line: "Subtitle and audio languages, size and a second line" },
  { id: "downloads", label: "Downloads", tone: "blue", icon: () => <ArrowDownToLine />, line: "Smart downloads and storage" },
  { id: "device", label: "This device", tone: "sea", icon: () => <Monitor />, line: "Settings kept on this device only" },
  { id: "about", label: "About", tone: "grey", icon: () => <Info />, line: "Version, credits and licences" },
];

/** Settings (A1, the owner's pick of 2026-10-28) in sections (plans/settings-sections.md, "On A1"). The profile sits
 * on a film's backdrop (the first Continue Watching title with one, else Home's first hero; a kids profile only its
 * own Continue Watching), with the film's logo at the right. TV and desktop: under it a section menu beside the
 * chosen section (`/settings/<id>`, Sources first), centered in the content column. The phone: the profile's edits,
 * then a row per section, each opening the section as its own page (`/settings/<id>`). */
export default function SettingsScreen() {
  const params = useParams<{ section?: string }>();
  const navigate = useNavigate();
  // The phone's section page, or the section beside the menu.
  const open = () => (form() === "phone" ? sectionId(params.section) : undefined);
  const chosen = () => sectionId(params.section) ?? "sources";
  // Incognito hides the account and Downloads' settings (plans/batch-10-incognito.md 6); the profile's own section is
  // its header, and Sources leaves out Trakt and Import.
  const hidden = (id: string) => incognito() && (id === "account" || id === "downloads");
  const listed = (s: { id: string }) => (s.id !== "downloads" || downloadsHere()) && !hidden(s.id);
  const shows = (id: string) => !hidden(id) && (form() !== "phone" ? id === chosen() : open() ? id === open() : id === "profiles");
  const leaving = useLeave();
  const [previews, setPreviews] = createSignal(previewsOn());
  const prefs = prefsView();
  const addons = liveView<AddonsSnapshot>("addons");
  // Nuvio plugin repositories (R7): the row shows only where plugins run (full builds, not kids).
  const [plugins] = createResource(() => op<PluginsAnswer>("plugins").catch(() => null));
  const home = homeView();
  const p = () => prefs.state.data;
  const [error, setError] = createSignal<string | null>(null);
  // Answers whether it was saved; a failure shows on the page.
  const patch = (x: Record<string, unknown>) => {
    setError(null);
    return op<Prefs>("setPrefs", { patch: x }).then(
      () => true,
      (e: unknown) => {
        setError(reasonOf(e));
        return false;
      },
    );
  };
  const me = () => current(profiles());
  const kids = () => me()?.kids === true;
  // Edit and Manage need the owner's PIN on a kids profile (DECISION kids rules): a kids profile shows neither.
  const canEdit = () => !!me() && !kids() && !incognito();
  const canManage = () => me()?.isOwner === true;
  // Sign in and Connect Google sit here, where people look for them; the Account page keeps the rest.
  // Signed out: the sync service this device knows, where it offers sign-in (the Account page's rule).
  const [offered] = createResource(() => !sync.joined() && (signInServer() || false), relayProviders);
  const signIn = () => !sync.joined() && offered.state === "ready" && offered().length > 0;
  // Joined, on the owner (D16) and never on a TV: the account's links and its relay's providers.
  const [linking] = createResource(
    () => sync.joined() && form() !== "tv" && mayDeleteAccount(profiles()) && (sync.accountServer() || false),
    async (server) => ({ offered: await relayProviders(server), linked: (await sync.identities()).map((l) => l.provider) }),
  );
  const connect = () => linking.state === "ready" && connectGoogle(linking().offered, linking().linked);
  // Decided once Home's rows are in, so a hero's art never shows first and gives way to Continue Watching's.
  const art = () =>
    home.rows.length === 0
      ? undefined
      : (home.rows.find((r) => r.rowId === CONTINUE_ROW)?.tiles.find((t) => t.background) ??
        (kids() ? undefined : home.heroes.find((h) => h.tile.background)?.tile));
  const logo = useTrimmedLogo(() => art()?.logo);
  // Warnings show here too, not only on the sync page: what failed or waits, then the time.
  const syncLine = () =>
    syncSummary({
      locked: sync.locked(),
      problem: sync.problem()?.words ?? sync.memoryProblem(),
      joined: sync.joined(),
      refused: sync.failed().refused,
      unreadable: sync.failed().unreadable,
      newer: sync.failed().newer,
      report: sync.report(),
      time: sync.syncedAt() ? new Date(sync.syncedAt() as number).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" }) : null,
    });

  let page!: HTMLElement;
  // A menu entry shows its section in place of the last (Profiles goes up to the header), and brings the column's top
  // into view. The remote goes on into the section, and stays on the menu where it has nothing to press (About); a
  // keyboard lands on its heading, or on the profile for the header.
  const jump = (e: MouseEvent, id: string) => {
    e.preventDefault();
    if (id !== "profiles") navigate(`/settings/${id}`, { replace: true });
    requestAnimationFrame(() => {
      const sec = document.getElementById(`set-${id}`);
      if (!sec) return;
      const behavior = reducedMotion() ? "auto" : "smooth";
      if (id === "profiles") page.scrollTo({ top: 0, behavior });
      else if (sec.getBoundingClientRect().top < page.getBoundingClientRect().top) sec.scrollIntoView({ block: "start", behavior });
      const to = form() === "tv" || id === "profiles" ? sec.querySelector<HTMLElement>("[data-f]") : sec.querySelector<HTMLElement>("h2[tabindex]");
      if (to) form() === "tv" ? focusEl(to) : to.focus({ preventScroll: true });
    });
  };

  const header = (
    <header class="set-head" id="set-profiles" data-home={home.heroes.length && home.rows.length ? "ready" : home.status.state}>
      <Show when={art()?.background} keyed>
        {(src) => <Backdrop src={src} class="set-art" ramp={form() === "phone" ? "settingsPhone" : "settings"} />}
      </Show>
      <h1 class="set-title">Settings</h1>
      <Show when={me()}>
        {(m) => (
          <div class="set-who-row">
            <A class="set-who" href="/profiles" data-f="settings:profiles">
              <AvatarArt avatar={m().avatar} name={m().name} size="l" />
              <div class="set-name">
                <h2>{m().name}</h2>
                <small>
                  {incognito() ? incognitoWords.kept : (profiles()?.profiles.length ?? 1) > 1 ? accountWords.switchProfile : "Add a profile for someone else, or switch"}
                </small>
              </div>
              <ChevronRight aria-hidden class="chev" />
            </A>
            {/* Incognito: Leave is the header's one button, beside the block that opens the chooser. */}
            <Show when={incognito()}>
              <div class="acts set-acts">
                <button class="btn" data-f="settings:leave-incognito" aria-disabled={leaving.busy()} onClick={leaving.leave}>
                  <LogOut aria-hidden />
                  {incognitoWords.leave}
                </button>
              </div>
              <Show when={leaving.problem()}>
                {(e) => (
                  <p class="st bad" role="status">
                    <i class="dot" />
                    {e()}
                  </p>
                )}
              </Show>
            </Show>
            <Show when={form() !== "phone" && canEdit()}>
              <div class="acts set-acts">
                <A class="btn" href={`/profiles/edit/${m().id}`} data-f="settings:profile-edit">
                  <UserPen aria-hidden />
                  Edit profile
                </A>
                <Show when={canManage()}>
                  <A class="btn" href="/profiles/manage" data-f="settings:profiles-manage">
                    <Users aria-hidden />
                    Manage profiles
                  </A>
                </Show>
              </div>
            </Show>
          </div>
        )}
      </Show>
      {/* The logo's box is kept from the start while its margins are cut, so the words beside it never move. */}
      <Show when={form() !== "phone" && art()?.logo}>
        <img class="set-logo" classList={{ pending: !logo() }} src={logo()?.src} alt="" />
      </Show>
    </header>
  );

  return (
    <main class="browse page settings-page a1" classList={{ "sec-page": !!open() }} data-vscroll ref={page}>
      <Show when={open()} fallback={header}>
        {(id) => (
          <h1 class="set-page-title" tabindex="-1">
            {SECTIONS.find((s) => s.id === id())?.label}
          </h1>
        )}
      </Show>
      <div class="set-body">
        <Show when={form() !== "phone"}>
          <nav class="set-menu" aria-label="Settings sections" data-fgroup="settings-menu">
            <span class="set-menu-title" aria-hidden="true">
              Settings
            </span>
            <For each={SECTIONS.filter(listed)}>
              {(s) => (
                <a
                  class="snav"
                  href={s.id === "profiles" ? "/settings" : `/settings/${s.id}`}
                  data-f={`settings-nav:${s.id}`}
                  aria-current={chosen() === s.id ? "true" : undefined}
                  onClick={(e) => jump(e, s.id)}
                >
                  <Tile tone={s.tone}>{s.icon()}</Tile>
                  {s.label}
                </a>
              )}
            </For>
          </nav>
        </Show>
        <div class="set-list">
          <Show when={error()}>
            <p class="st bad" role="status">
              <i class="dot" />
              {error()}
            </p>
          </Show>

          {/* The phone's header holds the profile alone; its edits are a group of their own, then a row per section. */}
          <Show when={form() === "phone" && !open() && canEdit()}>
            <section class="set-sec" aria-labelledby="set-h-profiles">
              <h2 id="set-h-profiles" tabindex="-1">
                Profiles
              </h2>
              <div class="sgroup">
                <Row href={`/profiles/edit/${me()?.id}`} fkey="settings:profile-edit" tone="blue" icon={<UserPen />} title="Edit profile" text="Name, picture and PIN." />
                <Show when={canManage()}>
                  <Row href="/profiles/manage" fkey="settings:profiles-manage" tone="sky" icon={<Users />} title="Manage profiles" text="Add, edit and remove profiles on this account." />
                </Show>
              </div>
            </section>
          </Show>

          <Show when={form() === "phone" && !open()}>
            <section class="set-sec" aria-label="Sections">
              <div class="sgroup">
                <For each={SECTIONS.filter((s) => s.id !== "profiles" && listed(s))}>
                  {(s) => <Row href={`/settings/${s.id}`} fkey={`settings-sec:${s.id}`} tone={s.tone} icon={s.icon()} title={s.label} text={s.line} />}
                </For>
              </div>
            </section>
          </Show>

          <Show when={shows("sources")}>
            <section class="set-sec" id="set-sources" aria-labelledby="set-h-sources">
              <h2 id="set-h-sources" tabindex="-1">
                Sources
              </h2>
              <Show
                when={!incognito()}
                fallback={
                  <>
                    {/* Incognito: its own copy of the addons and plugins; the device's debrid account is used, never changed. */}
                    <div class="sgroup">
                      <Row href="/addons" fkey="settings:addons" tone="mint" icon={<Puzzle />} title="Addons" text={addonsSummary(addons.state.data?.addons ?? [])} />
                      <Show when={plugins()?.available}>
                        <Row href="/plugins" fkey="settings:plugins" tone="teal" icon={<Plug />} title={pluginWords.title} text={pluginsSummary(plugins()?.repos ?? [])} />
                      </Show>
                    </div>
                    <Show when={debridAccounts()?.accounts.length}>
                      <p class="note small">{incognitoWords.debrid}</p>
                    </Show>
                  </>
                }
              >
              <div class="sgroup">
                <Row
                  href="/recommended-addons"
                  fkey="settings:recommended"
                  tone="green"
                  icon={<Sparkles />}
                  title={defaultsWords.title}
                  text="Catalogs, streams, subtitles and lists Momotus recommends, a switch each."
                />
                <Row href="/find-addons" fkey="settings:find-addons" tone="teal" icon={<PackageSearch />} title={findAddons.title} text={findAddons.lede} />
                <Row
                  href="/addons"
                  fkey="settings:addons"
                  tone="mint"
                  icon={<Puzzle />}
                  title="Addons"
                  text={addonsSummary(addons.state.data?.addons ?? [])}
                />
                <Show when={plugins()?.available}>
                  <Row href="/plugins" fkey="settings:plugins" tone="teal" icon={<Plug />} title={pluginWords.title} text={pluginsSummary(plugins()?.repos ?? [])} />
                </Show>
                <Row
                  href="/debrid"
                  fkey="settings:debrid"
                  tone="sky"
                  icon={<KeyRound />}
                  title="Debrid services"
                  text={debridAccounts()?.accounts.length ? inUse(debridAccounts()!.accounts.length) : "Play torrents as fast direct links with a debrid account."}
                />
                <Row
                  href="/trakt"
                  fkey="settings:trakt"
                  tone="blue"
                  icon={<Radio />}
                  title={traktWords.title}
                  text={rowLine(traktStatus(), traktLink, clockTime, traktStorage()?.words ?? null, traktLockedHere())}
                />
                <Row href="/import" fkey="settings:import" tone="sea" icon={<Download />} title="Import from Stremio" text="Addons, library and progress, with a code. No password typed here." />
                <Row href="/welcome?step=3" fkey="settings:setup" tone="grey" icon={<Wrench />} title="Setup check" text="Network, display, catalogs, sources and subtitles at a glance." />
              </div>
              </Show>
            </section>
          </Show>

          <Show when={shows("account")}>
            <section class="set-sec" id="set-account" aria-labelledby="set-h-account">
              <h2 id="set-h-account" tabindex="-1">
                Account
              </h2>
              <div class="sgroup">
                {/* A TV signs in with the phone's QR, from the first-run sign-in. */}
                <Show when={signIn()}>
                  <Row href={form() === "tv" ? "/welcome" : "/sign-in"} fkey="settings:sign-in" tone="blue" icon={<LogIn />} title={accountWords.signInTitle} text={accountWords.libraryAdded} />
                </Show>
                <Row href="/sync" fkey="settings:sync" tone="sky" icon={<MonitorSmartphone />} title={syncWords.title} text={syncLine()} />
                {/* The Account page opens with Google's button ready: its window needs that page's own click. */}
                <Show when={connect()}>
                  <Row href="/sync?connect=google" fkey="settings:connect-google" tone="blue" icon={<Link2 />} title={connectTitle("google")} text={connectText("google")} />
                </Show>
              </div>
            </section>
          </Show>

          <Show when={shows("home")}>
            <section class="set-sec" id="set-home" aria-labelledby="set-h-home">
              <h2 id="set-h-home" tabindex="-1">
                Home
              </h2>
              <div class="sgroup">
                <Row href="/home-rows" fkey="settings:home-rows" tone="mint" icon={<Rows3 />} title="Home rows" text="Which catalogs Home shows after Continue Watching, and in what order." />
              </div>
            </section>
          </Show>

          <Show when={shows("playback")}>
            <section class="set-sec" id="set-playback" aria-labelledby="set-h-playback">
              <h2 id="set-h-playback" tabindex="-1">
                Playback
              </h2>
              <Show when={p()}>
                {(x) => (
                  <>
                    <div class="sgroup">
                      <Row
                        href="/stream-choice"
                        fkey="settings:stream-choice"
                        tone="teal"
                        icon={<SlidersHorizontal />}
                        title={choiceWords.settings.title}
                        text={choiceSummary(x().streams)}
                      />
                    </div>
                    <div class="sgroup filters">
                      <Choice<boolean>
                        label="Up next"
                        group="pref-next"
                        items={[
                          { value: true, label: "Starts on its own" },
                          { value: false, label: "Wait for me" },
                        ]}
                        isOn={(v) => x().autoplayNext === v}
                        onPick={(v) => patch({ autoplayNext: v })}
                      />
                      {/* This device only, never synced (plans/appletv-polish.md decision 13). A TV shows it where its title pages
                          may play the header trailer (plans/trailer-inline.md 4a); its Home has none. */}
                      <Show when={form() !== "tv" || headerOnTvNow()}>
                        <Choice<boolean>
                          label="Trailer previews"
                          group="pref-previews"
                          items={[
                            { value: true, label: "On" },
                            { value: false, label: "Off" },
                          ]}
                          isOn={(v) => previews() === v}
                          onPick={(v) => {
                            setPreviewsOn(v);
                            setPreviews(v);
                          }}
                        />
                      </Show>
                    </div>
                    {/* The switch keeps its value; Incognito plays no trailer (plans/batch-10-incognito.md 5.5). */}
                    <Show when={incognito() && (form() !== "tv" || headerOnTvNow())}>
                      <p class="note small">{incognitoWords.trailersOff}</p>
                    </Show>
                    <Show when={form() !== "tv" || headerOnTvNow()}>
                      <p class="note small">Trailers play muted on {form() === "tv" ? "title pages" : "Home and title pages"}, from YouTube, when the connection is not metered.</p>
                    </Show>
                  </>
                )}
              </Show>
            </section>
          </Show>

          <Show when={shows("subtitles")}>
            <section class="set-sec" id="set-subtitles" aria-labelledby="set-h-subtitles">
              <h2 id="set-h-subtitles" tabindex="-1">
                Subtitles and audio
              </h2>
              <Show when={p()}>
                {(x) => (
                  <>
                    <div class="sgroup filters">
                      <LanguagePicker
                        label="Subtitle language"
                        none="None, subtitles start off"
                        group="pref-subs"
                        chosen={x().subtitleLanguages}
                        onChange={(next) => patch({ subtitleLanguages: next })}
                      />
                      <Choice<SubtitleSize>
                        label="Subtitle size"
                        group="pref-subsize"
                        items={[
                          { value: "small", label: "Small" },
                          { value: "medium", label: "Medium" },
                          { value: "large", label: "Large" },
                          { value: "extraLarge", label: "Extra large" },
                        ]}
                        isOn={(v) => x().subtitleSize === v}
                        onPick={(v) => patch({ subtitleSize: v })}
                      />
                      <LanguagePicker
                        audio
                        label={choiceWords.settings.audio.label}
                        help={choiceWords.settings.audio.help}
                        group="choice-audio"
                        chosen={x().streams.languages}
                        onChange={(next) => patch({ streams: { languages: next } })}
                      />
                    </div>
                    {/* Dual subtitles (plans/dual-subtitles.md 5.5): the same profile values as the player's Second line tab. */}
                    <div class="sgroup filters">
                      <SecondLine chosen={x().secondSubtitle} first={x().subtitleLanguages[0]} onPick={(v) => patch({ secondSubtitle: v })} />
                      <Show when={x().secondSubtitle !== null}>
                        <Choice<SubtitleLayout>
                          label={dual.layout}
                          help="Stacked puts the second line above the first, Split at the top of the picture, One box both in one box."
                          group="pref-layout"
                          items={(["stacked", "split", "oneBox"] as const).map((value) => ({ value, label: dual.layouts[value] }))}
                          isOn={(v) => x().subtitleLayout === v}
                          onPick={(v) => patch({ subtitleLayout: v })}
                        />
                        <Choice<boolean>
                          label={dual.onlyWhenPaused}
                          help="The second line shows only while the video is paused."
                          group="pref-paused"
                          items={[
                            { value: true, label: "On" },
                            { value: false, label: "Off" },
                          ]}
                          isOn={(v) => x().secondOnPause === v}
                          onPick={(v) => patch({ secondOnPause: v })}
                        />
                      </Show>
                    </div>
                    <p class="note small">The numbers are the order you turned languages on; number 1 wins. A subtitle you pick while watching makes its language number 1.</p>
                  </>
                )}
              </Show>
            </section>
          </Show>

          <Show when={downloadsHere() && shows("downloads")}>
            <DownloadsSection />
          </Show>

          <Show when={shows("device")}>
            <section class="set-sec" id="set-device" aria-labelledby="set-h-device">
              <h2 id="set-h-device" tabindex="-1">
                This device
              </h2>
              <div class="sgroup filters">
                <Choice<Form | null>
                  label="Layout"
                  group="dev-form"
                  items={[
                    { value: null, label: "Automatic" },
                    { value: "tv", label: "TV" },
                    { value: "desktop", label: "Desktop" },
                    { value: "phone", label: "Phone" },
                  ]}
                  isOn={(v) => pinned() === v}
                  onPick={(v) => chooseForm(v)}
                />
              </div>
            </section>
          </Show>

          <Show when={shows("about")}>
            <section class="set-sec" id="set-about" aria-labelledby="set-h-about">
              <h2 id="set-h-about" tabindex="-1">
                About
              </h2>
              <div class="sgroup notes">
                <p class="note">The name is drawn from Unbounded, SIL Open Font License 1.1.</p>
                <p class="note credit">
                  <img class="credit-mark" src={markSrc("tmdb")} alt="" />
                  {tmdbCredit}
                </p>
              </div>
            </section>
          </Show>
        </div>
      </div>
    </main>
  );
}

/** Settings > Downloads (plans/offline-downloads.md 3.7): Smart downloads, the storage line and Delete
 * all downloads with its inline confirmation. "Download on Wi-Fi only" is not offered: a browser sends
 * no cost flag, so its network is never metered. */
function DownloadsSection() {
  const view = liveView<DownloadsSnapshot>("downloads");
  const [confirm, setConfirm] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  return (
    <Show when={shownDownloads(view.state.data)}>
      {(s) => (
        <section class="set-sec dl-settings" id="set-downloads" aria-labelledby="set-h-downloads">
          <h2 id="set-h-downloads" tabindex="-1">
            Downloads
          </h2>
          <div class="sgroup">
            <button
              class="drow"
              role="switch"
              aria-checked={s().settings.smart}
              data-f="settings:smart-downloads"
              onClick={() => {
                setError(null);
                op("setDownloadSettings", { smart: !s().settings.smart }).catch((e) => setError(reasonOf(e)));
              }}
            >
              <span class="dlogo" aria-hidden="true">
                <Sparkles />
              </span>
              <span class="dtext">
                <b>{downloadWords.smart}</b>
                <small>{downloadWords.smartLine}</small>
              </span>
              <span class="switch" aria-hidden="true">
                <i />
              </span>
              <span class="dword" aria-hidden="true">
                {s().settings.smart ? "On" : "Off"}
              </span>
            </button>
          </div>
          <Show when={error()}>
            {(e) => (
              <p class="st bad" role="status">
                <i class="dot" />
                {e()}
              </p>
            )}
          </Show>
          <StorageLine storage={s().storage} />
          <Show when={s().storage.downloadsBytes > 0}>
            <div class="dl-confirm" role="group" aria-label={confirm() ? deleteAll(s().storage.downloadsBytes) : undefined}>
              <Show
                when={confirm()}
                fallback={
                  <button
                    class="btn"
                    data-f="settings:delete-downloads"
                    onClick={() => {
                      setConfirm(true);
                      focusOn("settings:delete-downloads-cancel");
                    }}
                  >
                    <Trash aria-hidden />
                    Delete all downloads
                  </button>
                }
              >
                {/* DECISION's Confirmation pattern, inline: no destructive red; the focus waits on Cancel. */}
                <p>{deleteAll(s().storage.downloadsBytes)}</p>
                <button
                  class="btn"
                  data-f="settings:delete-downloads-confirm"
                  onClick={() => {
                    setConfirm(false);
                    downloadAct("downloadRemove", { all: true });
                  }}
                >
                  Delete
                </button>
                <button
                  class="btn"
                  data-f="settings:delete-downloads-cancel"
                  onClick={() => {
                    setConfirm(false);
                    focusOn("settings:delete-downloads");
                  }}
                >
                  Cancel
                </button>
              </Show>
            </div>
          </Show>
        </section>
      )}
    </Show>
  );
}
