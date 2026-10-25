import { A } from "@solidjs/router";
import Check from "lucide-solid/icons/check";
import ChevronRight from "lucide-solid/icons/chevron-right";
import Download from "lucide-solid/icons/download";
import KeyRound from "lucide-solid/icons/key-round";
import PackageSearch from "lucide-solid/icons/package-search";
import Puzzle from "lucide-solid/icons/puzzle";
import MonitorSmartphone from "lucide-solid/icons/monitor-smartphone";
import Radio from "lucide-solid/icons/radio";
import SearchIcon from "lucide-solid/icons/search";
import Rows3 from "lucide-solid/icons/rows-3";
import SlidersHorizontal from "lucide-solid/icons/sliders-horizontal";
import Sparkles from "lucide-solid/icons/sparkles";
import Trash from "lucide-solid/icons/trash";
import ArrowDownToLine from "lucide-solid/icons/arrow-down-to-line";
import CirclePlay from "lucide-solid/icons/circle-play";
import Info from "lucide-solid/icons/info";
import Monitor from "lucide-solid/icons/monitor";
import User from "lucide-solid/icons/user";
import Wrench from "lucide-solid/icons/wrench";
import { createSignal, For, type JSX, onCleanup, onMount, Show } from "solid-js";
import { deviceLanguage, homeView, liveView, op, prefsView, reasonOf } from "../core/client";
import { Backdrop, useTrimmedLogo } from "../components/art";
import { CONTINUE_ROW } from "../components/row";
import { focusEl, reducedMotion } from "../focus/dom";
import type { AddonsSnapshot } from "../core/generated/AddonsSnapshot";
import type { DownloadsSnapshot } from "../core/generated/DownloadsSnapshot";
import { downloadAct, downloadsHere } from "../components/download";
import { deleteAll, downloadWords, shownDownloads } from "../lib/download";
import { focusOn, StorageLine } from "./downloads";
import type { LanguageChoice } from "../core/generated/LanguageChoice";
import type { Prefs } from "../core/generated/Prefs";
import type { SubtitleSize } from "../core/generated/SubtitleSize";
import { chooseForm, type Form, form, pinned } from "../form";
import { headerOnTvNow, previewsOn, setPreviewsOn } from "../lib/trailers";
import { defaultsWords } from "../lib/defaults";
import { addonsSummary, choiceSummary, choiceWords, findAddons, languageName, syncSummary, syncWords, tmdbCredit, turkceAltyaziCredit } from "../lib/words";
import * as sync from "../core/sync";
import { traktLink, traktLockedHere, traktStatus, traktStorage } from "../core/trakt";
import { rowLine, traktWords } from "../lib/trakt";
import "./browse.css";
import { markSrc } from "../components/marks";
import "./settings.css";
import "./onboarding.css";
import UserPen from "lucide-solid/icons/user-pen";
import Users from "lucide-solid/icons/users";
import { AvatarArt } from "../components/avatar";
import { profiles } from "../core/profiles";
import { debridAccounts } from "../core/debrid";
import { inUse } from "../lib/debrid";
import { accountWords } from "../lib/account";

const clockTime = (ms: number) => new Date(ms).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });

/** The languages the pickers show before "More languages", as core tags. */
export const QUICK = ["en", "tr", "es", "fr", "de", "it", "pt", "ja"];
/** The ISO 639-2 codes earlier pickers stored for subtitles; the core reads both. */
const LEGACY: Record<string, string> = { eng: "en", tur: "tr", spa: "es", fre: "fr", ger: "de", ita: "it", por: "pt", jpn: "ja" };

// Every language the core knows (113), loaded once per app run; `device`
// marks the one this browser's language reads as.
let catalog: Promise<LanguageChoice[]> | undefined;
export const allLanguages = () =>
  (catalog ??= op<LanguageChoice[]>("languages", { device: deviceLanguage() }).catch((e: unknown) => {
    catalog = undefined;
    throw e;
  }));

/** A language picker (subtitles; audio with `audio`): the common languages,
 * then the other chosen ones, each chosen one numbered by its rank (the
 * first one wins), and every language the core knows behind "More
 * languages", with a search. Chips keep their place and their element when
 * one is picked, so a remote's focus stays on it. Subtitles store the core's
 * tag as is, audio the part before "-" ("pt" for "pt-BR"). `first` words the
 * help line from the first chosen language's name (null for none). */
export function LanguagePicker(props: {
  label: string;
  help?: string;
  first?: (name: string | null) => string;
  group: string;
  chosen: string[];
  audio?: boolean;
  onChange: (next: string[]) => void;
}) {
  const [all, setAll] = createSignal<LanguageChoice[] | null>(null);
  const [failed, setFailed] = createSignal<string | null>(null);
  const [more, setMore] = createSignal(false);
  const [query, setQuery] = createSignal("");
  onMount(() => void allLanguages().then(setAll, (e: unknown) => setFailed(reasonOf(e))));
  const codeOf = (tag: string) => (props.audio ? (tag.split("-")[0] ?? tag) : tag);
  const norm = (v: string) => codeOf(LEGACY[v.toLowerCase()] ?? v);
  const chosen = () => props.chosen.map(norm);
  const nameOf = (code: string) => all()?.find((l) => l.code === code)?.name ?? languageName(code);
  // One object per language, so `For` keeps each chip's element.
  const items = new Map<string, { value: string; label: string }>();
  const item = (code: string) => {
    const label = nameOf(code);
    let it = items.get(code);
    if (it?.label !== label) items.set(code, (it = { value: code, label }));
    return it;
  };
  const shown = () => [...new Set([...QUICK.map(codeOf), ...chosen()])].map(item);
  const rank = (code: string) => chosen().indexOf(code) + 1;
  const options = () => {
    const q = query().trim().toLowerCase();
    const codes = [...new Set((all() ?? []).map((l) => codeOf(l.code)))];
    return codes
      .map(item)
      .filter((i) => !q || i.label.toLowerCase().includes(q))
      .sort((a, b) => a.label.localeCompare(b.label, "en"));
  };
  const isOn = (code: string) => chosen().includes(code);
  const pick = (code: string) => props.onChange(isOn(code) ? props.chosen.filter((v) => norm(v) !== code) : [...props.chosen, code]);
  return (
    <Choice<string>
      multi
      label={props.label}
      help={props.first ? props.first(chosen()[0] ? nameOf(chosen()[0] as string) : null) : props.help}
      group={props.group}
      items={shown()}
      isOn={isOn}
      rank={rank}
      onPick={pick}
      extra={
        <>
          <button class="chip" aria-expanded={more()} data-f={`${props.group}:more`} onClick={() => setMore(!more())}>
            {more() ? "Fewer languages" : "More languages"}
          </button>
          <Show when={more()}>
            <div class="lang-more">
              <label class="lang-search">
                <SearchIcon aria-hidden />
                <span class="sr">Search languages</span>
                <input
                  type="search"
                  placeholder="Search languages"
                  autocomplete="off"
                  spellcheck={false}
                  data-f={`${props.group}:search`}
                  value={query()}
                  onInput={(e) => setQuery(e.currentTarget.value)}
                />
              </label>
              <Show when={failed()}>{(why) => <p class="note small">The language list could not be loaded: {why()}</p>}</Show>
              <Show when={all() && options().length === 0}>
                <p class="note small">No language is called "{query().trim()}".</p>
              </Show>
              <div class="lang-all">
                <For each={options()}>{(o) => <ChoiceChip group={`${props.group}-all`} item={o} on={isOn(o.value)} rank={rank(o.value)} multi onPick={pick} />}</For>
              </div>
            </div>
          </Show>
        </>
      }
    />
  );
}

function ChoiceChip<T>(props: { group: string; item: { value: T; label: string }; on: boolean; rank?: number; multi?: boolean; autofocus?: boolean; onPick: (v: T) => void }) {
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
        <Show when={props.rank} fallback={<Check aria-hidden class="chip-check" />}>
          {(n) => <b class="chip-rank num">{n()}</b>}
        </Show>
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
  /** A chosen item's rank (1 first), shown in place of the check. */
  rank?: (v: T) => number;
  onPick: (v: T) => void;
  autofocus?: boolean;
  multi?: boolean;
  /** After the chips, in the same track ("More languages"). */
  extra?: JSX.Element;
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
              rank={props.rank?.(item.value)}
              multi={props.multi}
              autofocus={props.autofocus && i() === 0}
              onPick={props.onPick}
            />
          )}
        </For>
        {props.extra}
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

/** The section menu's entries, in page order. Profiles is the header. */
const SECTIONS: { id: string; label: string; tone: Tone; icon: () => JSX.Element }[] = [
  { id: "profiles", label: "Profiles", tone: "blue", icon: () => <User /> },
  { id: "sources", label: "Sources", tone: "green", icon: () => <Puzzle /> },
  { id: "account", label: "Account", tone: "sky", icon: () => <MonitorSmartphone /> },
  { id: "home", label: "Home", tone: "mint", icon: () => <Rows3 /> },
  { id: "playback", label: "Playback", tone: "teal", icon: () => <CirclePlay /> },
  { id: "downloads", label: "Downloads", tone: "blue", icon: () => <ArrowDownToLine /> },
  { id: "device", label: "This device", tone: "sea", icon: () => <Monitor /> },
  { id: "about", label: "About", tone: "grey", icon: () => <Info /> },
];

/** Settings (A1, the owner's pick of 2026-10-28): one long page. The profile sits on a film's backdrop (the first
 * Continue Watching title with one, else Home's first hero; a kids profile only its own Continue Watching), with
 * the film's logo at the right; under it a section menu (TV and desktop) beside the grouped lists, centered in the
 * content column. The phone has no menu: its groups follow each other under the header. */
export default function SettingsScreen() {
  const [previews, setPreviews] = createSignal(previewsOn());
  const prefs = prefsView();
  const addons = liveView<AddonsSnapshot>("addons");
  const home = homeView();
  const p = () => prefs.state.data;
  const [error, setError] = createSignal<string | null>(null);
  const patch = (x: Record<string, unknown>) => {
    setError(null);
    op<Prefs>("setPrefs", { patch: x }).catch((e) => setError(reasonOf(e)));
  };
  const me = () => profiles()?.profiles.find((x) => x.id === profiles()?.active);
  const kids = () => me()?.kids === true;
  // Edit and Manage need the owner's PIN on a kids profile (DECISION kids rules): a kids profile shows neither.
  const canEdit = () => !!me() && !kids();
  const canManage = () => me()?.isOwner === true;
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

  // The section menu fills the section at the top third of the page; the list's tail lets the last one get there.
  let page!: HTMLElement;
  const [active, setActive] = createSignal("profiles");
  const spy = () => {
    const line = page.getBoundingClientRect().top + page.getBoundingClientRect().height / 3;
    let on = "profiles";
    for (const s of SECTIONS) {
      const el = document.getElementById(`set-${s.id}`);
      if (el && el.getBoundingClientRect().top <= line) on = s.id;
    }
    setActive(on);
  };
  let frame = 0;
  // A menu entry's own scroll keeps its entry filled (a short section may never reach the line); the viewer's next
  // scroll, by wheel, touch, key or pointer, hands the menu back to the line.
  let held = false;
  const release = () => (held = false);
  const onScroll = () => {
    if (held) return;
    cancelAnimationFrame(frame);
    frame = requestAnimationFrame(spy);
  };
  onCleanup(() => cancelAnimationFrame(frame));
  // A menu entry brings its section to the top (Profiles, the header under the desktop bar, the page's top). The
  // remote goes on into the section, and stays on the menu where it has nothing to press (About); a keyboard lands
  // on its heading, or on the profile for the header.
  const jump = (e: MouseEvent, id: string) => {
    e.preventDefault();
    const sec = document.getElementById(`set-${id}`);
    if (!sec) return;
    const behavior = reducedMotion() ? "auto" : "smooth";
    if (id === "profiles") page.scrollTo({ top: 0, behavior });
    else sec.scrollIntoView({ block: "start", behavior });
    held = true;
    setActive(id);
    const to = form() === "tv" || id === "profiles" ? sec.querySelector<HTMLElement>("[data-f]") : sec.querySelector<HTMLElement>("h2[tabindex]");
    if (to) form() === "tv" ? focusEl(to) : to.focus({ preventScroll: true });
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
                <small>{(profiles()?.profiles.length ?? 1) > 1 ? accountWords.switchProfile : "Add a profile for someone else, or switch"}</small>
              </div>
              <ChevronRight aria-hidden class="chev" />
            </A>
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
    <main class="browse page settings-page a1" data-vscroll ref={page} onScroll={onScroll} onWheel={release} onTouchStart={release} onKeyDown={release} onPointerDown={release}>
      {header}
      <div class="set-body">
        <Show when={form() !== "phone"}>
          <nav class="set-menu" aria-label="Settings sections" data-fgroup="settings-menu">
            <span class="set-menu-title" aria-hidden="true">
              Settings
            </span>
            <For each={SECTIONS.filter((s) => s.id !== "downloads" || downloadsHere())}>
              {(s) => (
                <a class="snav" href={`#set-${s.id}`} data-f={`settings-nav:${s.id}`} aria-current={active() === s.id ? "true" : undefined} onClick={(e) => jump(e, s.id)}>
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

          {/* The phone's header holds the profile alone; its edits are a group of their own. */}
          <Show when={form() === "phone" && canEdit()}>
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

          <section class="set-sec" id="set-sources" aria-labelledby="set-h-sources">
            <h2 id="set-h-sources" tabindex="-1">
              Sources
            </h2>
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
          </section>

          <section class="set-sec" id="set-account" aria-labelledby="set-h-account">
            <h2 id="set-h-account" tabindex="-1">
              Account
            </h2>
            <div class="sgroup">
              <Row href="/sync" fkey="settings:sync" tone="sky" icon={<MonitorSmartphone />} title={syncWords.title} text={syncLine()} />
            </div>
          </section>

          <section class="set-sec" id="set-home" aria-labelledby="set-h-home">
            <h2 id="set-h-home" tabindex="-1">
              Home
            </h2>
            <div class="sgroup">
              <Row href="/home-rows" fkey="settings:home-rows" tone="mint" icon={<Rows3 />} title="Home rows" text="Which catalogs Home shows after Continue Watching, and in what order." />
            </div>
          </section>

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
                    <LanguagePicker
                      label="Subtitle language"
                      first={(name) => (name ? `${name} first` : "None, subtitles start off")}
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
                  <p class="note small">The numbers are the order you turned languages on; number 1 wins. A subtitle you pick while watching makes its language number 1.</p>
                  <Show when={form() !== "tv" || headerOnTvNow()}>
                    <p class="note small">Trailers play muted on {form() === "tv" ? "title pages" : "Home and title pages"}, from YouTube, when the connection is not metered.</p>
                  </Show>
                </>
              )}
            </Show>
          </section>

          <Show when={downloadsHere()}>
            <DownloadsSection />
          </Show>

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

          <section class="set-sec" id="set-about" aria-labelledby="set-h-about">
            <h2 id="set-h-about" tabindex="-1">
              About
            </h2>
            <div class="sgroup notes">
              <p class="note">The name is drawn from Unbounded, SIL Open Font License 1.1.</p>
              <p class="note">{turkceAltyaziCredit}</p>
              <p class="note credit">
                <img class="credit-mark" src={markSrc("tmdb")} alt="" />
                {tmdbCredit}
              </p>
            </div>
          </section>
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
