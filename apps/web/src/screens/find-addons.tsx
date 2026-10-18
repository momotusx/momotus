import { A } from "@solidjs/router";
import Check from "lucide-solid/icons/check";
import ExternalLink from "lucide-solid/icons/external-link";
import Info from "lucide-solid/icons/info";
import Link2 from "lucide-solid/icons/link-2";
import Plus from "lucide-solid/icons/plus";
import Power from "lucide-solid/icons/power";
import RotateCw from "lucide-solid/icons/rotate-cw";
import { createSignal, For, Match, Show, Switch } from "solid-js";
import { Chips } from "../components/grid";
import { liveView, op, reasonOf } from "../core/client";
import type { AddonCatalogSnapshot } from "../core/generated/AddonCatalogSnapshot";
import type { ListedAddon } from "../core/generated/ListedAddon";
import { form } from "../form";
import { cards, runCard } from "../lib/install-cards";
import { type Action, action, type Filter, filterEmpty, filters, initial, shows, spoken } from "../lib/find-addons";
import { findAddons as w, kindLabel, providesText } from "../lib/words";
import { InstallField } from "./addons";
import "./browse.css";
import "./settings.css";
import "./find-addons.css";

const install = (entry: ListedAddon) => runCard(entry.key, () => op("installListed", { key: entry.key }), reasonOf);
const turnOn = (entry: ListedAddon, url: string) => runCard(entry.key, () => op("setAddonEnabled", { url, enabled: true }), reasonOf);

const kindsWords: Record<string, string> = { movie: "movies", series: "series", channel: "channels", tv: "live TV", anime: "anime" };

/** "Subtitles for movies and series". */
function providesLine(entry: ListedAddon): string {
  const what = entry.provides.map(providesText).join(", ");
  const kinds = entry.types.map((t) => kindsWords[t] ?? kindLabel(t).toLowerCase());
  if (!kinds.length) return what;
  const last = kinds.length > 1 ? ` and ${kinds.pop()}` : "";
  return `${what} for ${kinds.join(", ")}${last}`;
}

function Card(props: { entry: ListedAddon; index: number; sameName: boolean }) {
  const e = () => props.entry;
  const tv = () => form() === "tv";
  const state = () => cards[e().key];
  // A failure can call for another action than the entry's: setup first.
  const act = (): Action => (state()?.code === "needsSetup" ? { kind: "configure" } : action(e(), tv()));
  const fkey = () => `fa:${e().id}:${e().host}:${props.index}`;
  const label = () => spoken(e(), act(), props.sameName, e().provides.map(providesText).join(", "));
  const perform = () => {
    const a = act();
    if (state()?.busy) return;
    if (a.kind === "install") void install(e());
    if (a.kind === "turnOn") void turnOn(e(), a.url);
  };
  return (
    <li
      class="fa-card"
      classList={{ busy: state()?.busy, done: act().kind === "none" }}
      data-fgroup={tv() ? undefined : fkey()}
      // On a TV the card is one focus stop and select performs its action.
      data-f={tv() ? fkey() : undefined}
      tabindex={tv() ? 0 : undefined}
      role={tv() && act().kind !== "none" && act().kind !== "configure" ? "button" : undefined}
      aria-label={tv() ? label() : undefined}
      aria-busy={state()?.busy || undefined}
      onClick={() => tv() && perform()}
    >
      <div class="fa-mark" aria-hidden="true">
        {initial(e().name)}
      </div>
      <div class="fa-text">
        <h2 dir="auto">{e().name}</h2>
        <p class="fa-host">{e().host}</p>
        <p class="fa-provides">{providesLine(e())}</p>
        <Show when={e().description}>
          <p class="fa-desc" dir="auto">
            {e().description}
          </p>
        </Show>
        <Show when={act().kind === "configure"}>
          <p class="fa-help">{tv() ? w.configureTv : w.configureHelp}</p>
        </Show>
        <Show when={!state()?.busy && state()?.words}>
          <p class="st bad" role="status">
            <i class="dot" />
            {state()?.words}
          </p>
        </Show>
      </div>
      <div class="fa-acts">
        <Switch>
          <Match when={state()?.busy}>
            <span class="fa-state live" role="status">
              <i class="spin" aria-hidden="true" />
              {w.installing}
            </span>
          </Match>
          <Match when={act().kind === "none" && (act() as { word: string })}>
            {(a) => (
              <span class="fa-state on">
                <Check aria-hidden />
                {a().word}
              </span>
            )}
          </Match>
          <Match when={tv()}>
            <Show when={act().kind === "install" || act().kind === "turnOn"}>
              <span class="fa-state">
                <Show when={act().kind === "turnOn"} fallback={<Plus aria-hidden />}>
                  <Power aria-hidden />
                </Show>
                {act().kind === "turnOn" ? w.turnOn : w.install}
              </span>
            </Show>
          </Match>
          <Match when={act().kind === "turnOn" && (act() as { url: string; word: string })}>
            {(a) => (
              <>
                <span class="fa-state">{a().word}</span>
                <button class="btn" data-f={`${fkey()}:on`} aria-label={label()} onClick={perform}>
                  <Power aria-hidden />
                  {w.turnOn}
                </button>
              </>
            )}
          </Match>
          <Match when={true}>
            <Show when={act().kind === "install"}>
              <button class="btn" data-f={`${fkey()}:install`} aria-label={label()} onClick={perform}>
                <Plus aria-hidden />
                {w.install}
              </button>
            </Show>
            <Show when={(act().kind === "configure" || (act() as { configure?: boolean }).configure) && e().configureUrl}>
              {(url) => (
                <a class="btn" href={url()} target="_blank" rel="noopener noreferrer" data-f={`${fkey()}:configure`}>
                  <ExternalLink aria-hidden />
                  {w.configure}
                </a>
              )}
            </Show>
          </Match>
        </Switch>
      </div>
    </li>
  );
}

function Lists(props: { fresh: boolean; onRetry: () => void }) {
  const view = liveView<AddonCatalogSnapshot>("addonCatalog", props.fresh ? { fresh: true } : {});
  const [list, setList] = createSignal("official");
  const [filter, setFilter] = createSignal<Filter>("all");
  const lists = () => view.state.data?.lists ?? [];
  const current = () => lists().find((l) => l.id === list());
  const entries = () => (current()?.entries ?? []).filter((e) => shows(e, filter()));
  const repeated = (entry: ListedAddon) => (current()?.entries ?? []).filter((e) => e.name === entry.name).length > 1;
  const failure = () => {
    const s = current()?.state;
    return s?.state === "failed" ? s.words : view.state.status.state === "failed" ? w.failed : null;
  };
  return (
    <>
      <div class="filters">
        <Chips
          label={w.listLabel}
          group="fa-lists"
          items={[
            { value: "official", label: w.official, note: lists().find((l) => l.id === "official")?.entries.length.toString() },
            { value: "community", label: w.communityList, note: lists().find((l) => l.id === "community")?.entries.length.toString() },
          ]}
          selected={list()}
          onSelect={setList}
          autofocus
        />
        <Chips label={w.showLabel} group="fa-filter" items={filters} selected={filter()} onSelect={setFilter} />
      </div>
      <Show when={list() === "community"}>
        <p class="fa-note">
          <Info aria-hidden />
          {w.community}
        </p>
      </Show>
      <Switch>
        <Match when={failure()}>
          {(words) => (
            <section class="problem" role="alert">
              <p>{words()}</p>
              <div class="acts">
                <button class="btn primary" data-f="fa:retry" data-autofocus onClick={props.onRetry}>
                  <RotateCw aria-hidden />
                  {w.tryAgain}
                </button>
              </div>
            </section>
          )}
        </Match>
        <Match when={!current() || current()?.state.state === "loading"}>
          <p class="st warn live fa-status" role="status">
            <i class="dot" />
            {w.loading}
          </p>
          <ul class="fa-grid" aria-hidden="true">
            <For each={Array.from({ length: 6 })}>{() => <li class="fa-card skel" />}</For>
          </ul>
        </Match>
        <Match when={current()?.entries.length === 0}>
          <p class="note">{w.empty}</p>
        </Match>
        <Match when={entries().length === 0 && filter() !== "all"}>
          <p class="note">{filterEmpty(filter() as Exclude<Filter, "all">)}</p>
        </Match>
        <Match when={true}>
          <ul class="fa-grid">
            <For each={entries()}>{(entry, i) => <Card entry={entry} index={i()} sameName={repeated(entry)} />}</For>
          </ul>
          <div class="fa-foot">
            <button class="btn" data-f="fa:refresh" onClick={props.onRetry}>
              <RotateCw aria-hidden />
              {w.refresh}
            </button>
          </div>
        </Match>
      </Switch>
    </>
  );
}

export default function FindAddons() {
  // Try again and Refresh close the view and open it with `fresh`, which
  // waits for the network instead of the addon cache.
  const [opens, setOpens] = createSignal(0);
  const tv = () => form() === "tv";
  return (
    <main class="browse page settings-page find-addons" data-vscroll>
      <h1 data-eyebrow={w.eyebrow}>{w.title}</h1>
      <p class="lede">{w.lede}</p>
      <Show
        when={!tv()}
        fallback={
          <A class="btn disclose" href="/addons" data-f="fa:paste">
            <Link2 aria-hidden />
            {w.pasteLink}
          </A>
        }
      >
        <InstallField fkey="fa-paste" />
      </Show>
      <Show when={opens() + 1} keyed>
        {(n) => <Lists fresh={n > 1} onRetry={() => setOpens(opens() + 1)} />}
      </Show>
    </main>
  );
}
