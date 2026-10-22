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
import Sparkle from "lucide-solid/icons/sparkle";
import Wrench from "lucide-solid/icons/wrench";
import { createSignal, For, type JSX, onMount, Show } from "solid-js";
import { deviceLanguage, liveView, op, prefsView, reasonOf } from "../core/client";
import type { AddonsSnapshot } from "../core/generated/AddonsSnapshot";
import type { LanguageChoice } from "../core/generated/LanguageChoice";
import type { Prefs } from "../core/generated/Prefs";
import type { SubtitleSize } from "../core/generated/SubtitleSize";
import { chooseForm, type Form, pinned } from "../form";
import { defaultsWords } from "../lib/defaults";
import { choiceSummary, choiceWords, findAddons, languageName, syncSummary, syncWords, turkceAltyaziCredit } from "../lib/words";
import * as sync from "../core/sync";
import { traktLink, traktLockedHere, traktStatus, traktStorage } from "../core/trakt";
import { rowLine, traktWords } from "../lib/trakt";
import "./browse.css";
import "./settings.css";
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

export default function SettingsScreen() {
  const prefs = prefsView();
  const addons = liveView<AddonsSnapshot>("addons");
  const p = () => prefs.state.data;
  const [error, setError] = createSignal<string | null>(null);
  const patch = (x: Record<string, unknown>) => {
    setError(null);
    op<Prefs>("setPrefs", { patch: x }).catch((e) => setError(reasonOf(e)));
  };
  const count = () => addons.state.data?.addons.length ?? 0;
  const sources = () => addons.state.data?.addons.filter((a) => a.enabled && a.provides.includes("stream")).map((a) => a.name) ?? [];
  const me = () => profiles()?.profiles.find((x) => x.id === profiles()?.active);
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

  return (
    <main class="browse page settings-page column" data-vscroll>
      <h1>Settings</h1>
      <Show when={error()}>
        <p class="st bad" role="status">
          <i class="dot" />
          {error()}
        </p>
      </Show>

      <Show when={me()}>
        {(m) => (
          <section class="set">
            <h2>Profiles</h2>
            <div class="links">
              <LinkRow
                href="/profiles"
                fkey="settings:profiles"
                icon={<AvatarArt avatar={m().avatar} name={m().name} size="s" />}
                title={m().name}
                text={(profiles()?.profiles.length ?? 1) > 1 ? accountWords.switchProfile : "Add a profile for someone else, or switch"}
              />
              <LinkRow href={`/profiles/edit/${m().id}`} fkey="settings:profile-edit" icon={<UserPen aria-hidden />} title="Edit profile" text="Name, picture and PIN." />
              <Show when={m().isOwner}>
                <LinkRow href="/profiles/manage" fkey="settings:profiles-manage" icon={<Users aria-hidden />} title="Manage profiles" text="Add, edit and remove profiles on this account." />
              </Show>
            </div>
          </section>
        )}
      </Show>

      <section class="set">
        <h2>Sources</h2>
        <div class="links">
          <LinkRow
            href="/recommended-addons"
            fkey="settings:recommended"
            icon={<Sparkle aria-hidden />}
            title={defaultsWords.title}
            text="Catalogs, streams, subtitles and lists Marquee recommends, a switch each."
          />
          <LinkRow href="/find-addons" fkey="settings:find-addons" icon={<PackageSearch aria-hidden />} title={findAddons.title} text={findAddons.lede} />
          <LinkRow
            href="/addons"
            fkey="settings:addons"
            icon={<Puzzle aria-hidden />}
            title="Addons"
            text={count() ? `${count()} installed. ${sources().length ? `Streams from ${sources().join(", ")}.` : "None of them finds streams yet."}` : "None installed yet."}
          />
          <LinkRow
            href="/debrid"
            fkey="settings:debrid"
            icon={<KeyRound aria-hidden />}
            title="Debrid services"
            text={debridAccounts()?.accounts.length ? inUse(debridAccounts()!.accounts.length) : "Play torrents as fast direct links with a debrid account."}
          />
          <LinkRow
            href="/trakt"
            fkey="settings:trakt"
            icon={<Radio aria-hidden />}
            title={traktWords.title}
            text={rowLine(traktStatus(), traktLink, clockTime, traktStorage()?.words ?? null, traktLockedHere())}
          />
          <LinkRow href="/import" fkey="settings:import" icon={<Download aria-hidden />} title="Import from Stremio" text="Addons, library and progress, with a code. No password typed here." />
          <LinkRow href="/welcome?step=3" fkey="settings:setup" icon={<Wrench aria-hidden />} title="Setup check" text="Network, display, catalogs, sources and subtitles at a glance." />
        </div>
      </section>

      <section class="set">
        <h2>Account</h2>
        <div class="links">
          <LinkRow href="/sync" fkey="settings:sync" icon={<MonitorSmartphone aria-hidden />} title={syncWords.title} text={syncLine()} />
        </div>
      </section>

      <section class="set">
        <h2>Home</h2>
        <div class="links">
          <LinkRow href="/home-rows" fkey="settings:home-rows" icon={<Rows3 aria-hidden />} title="Home rows" text="Which catalogs Home shows after Continue Watching, and in what order." />
        </div>
      </section>

      <Show when={p()}>
        {(x) => (
          <section class="set">
            <h2>Playback</h2>
            <div class="links">
              <LinkRow
                href="/stream-choice"
                fkey="settings:stream-choice"
                icon={<SlidersHorizontal aria-hidden />}
                title={choiceWords.settings.title}
                text={choiceSummary(x().streams)}
              />
            </div>
            <div class="filters">
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
            </div>
            <p class="note small">The numbers are the order you turned languages on; number 1 wins. A subtitle you pick while watching makes its language number 1.</p>
          </section>
        )}
      </Show>

      <section class="set">
        <h2>This device</h2>
        <div class="filters">
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

      <section class="set">
        <h2>About</h2>
        <p class="note">{turkceAltyaziCredit}</p>
      </section>
    </main>
  );
}
