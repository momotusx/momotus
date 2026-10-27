import { A } from "@solidjs/router";
import Check from "lucide-solid/icons/check";
import ExternalLink from "lucide-solid/icons/external-link";
import Info from "lucide-solid/icons/info";
import Link2 from "lucide-solid/icons/link-2";
import Plus from "lucide-solid/icons/plus";
import Power from "lucide-solid/icons/power";
import RotateCw from "lucide-solid/icons/rotate-cw";
import { createEffect, createMemo, createSignal, For, Match, on, onCleanup, Show, Switch } from "solid-js";
import { Chips } from "../components/grid";
import { liveView, op, reasonOf } from "../core/client";
import type { AddonCatalogSnapshot } from "../core/generated/AddonCatalogSnapshot";
import type { ListedAddon } from "../core/generated/ListedAddon";
import { onProfileReset } from "../core/profiles";
import { focusEl, isBackKey } from "../focus/dom";
import { form } from "../form";
import { type Action, cardAction, type Filter, filterEmpty, filters, initial, matches, providesLine, queryWords, repeatedNames, shows, spoken } from "../lib/find-addons";
import { cards, forgetAllCards, forgetCard, runCard } from "../lib/install-cards";
import { findAddons as w, providesText } from "../lib/words";
import { InstallField } from "./addons";
import "./browse.css";
import "./settings.css";
import "./find-addons.css";

// A card's state belongs to the profile that pressed it.
onProfileReset(forgetAllCards);

const install = (entry: ListedAddon) => runCard(entry.key, () => op("installListed", { key: entry.key }), reasonOf);
const turnOn = (entry: ListedAddon, url: string) => runCard(entry.key, () => op("setAddonEnabled", { url, enabled: true }), reasonOf);

/** Cards render in pages as the page scrolls toward them (Community has about 100). */
const PAGE = 24;

/** The list, filter and rendered count, kept in this history entry: Back to
 * the page shows the same cards, so focus finds the card it left. A new visit
 * opens on Official, All. */
type Picks = { list: string; filter: Filter; shown: number; query: string };
function savedPicks(): Picks {
  const p = (history.state as { findAddons?: Partial<Picks> } | null)?.findAddons;
  return { list: p?.list ?? "official", filter: p?.filter ?? "all", shown: p?.shown ?? PAGE, query: p?.query ?? "" };
}

function Card(props: { entry: ListedAddon; fkey: string; sameName: boolean }) {
  const e = () => props.entry;
  const tv = () => form() === "tv";
  const state = () => cards[e().key];
  const busy = () => state()?.busy === true;
  const act = createMemo((): Action => cardAction(e(), state()?.code));
  // A result is about the state it was pressed in: a new installed state
  // (from this card or another device) clears it.
  createEffect(on(() => e().installed, () => forgetCard(e().key), { defer: true }));
  const failure = () => (busy() || act().kind === "none" ? undefined : state()?.words);
  const provides = () => e().provides.map(providesText).join(", ");
  const label = (a: Action) => spoken(e(), a, props.sameName, provides());
  const word = () => {
    const a = act();
    return a.kind === "none" || a.kind === "turnOn" ? a.word : null;
  };
  const verb = () => (act().kind === "turnOn" ? w.turnOn : w.install);
  const busyWord = () => (act().kind === "turnOn" ? w.turningOn : w.installing);
  const perform = () => {
    const a = act();
    if (busy()) return;
    if (a.kind === "install") void install(e());
    if (a.kind === "turnOn") void turnOn(e(), a.url);
  };
  const pressable = () => act().kind === "install" || act().kind === "turnOn";
  const configureUrl = () => {
    const a = act();
    return a.kind === "configure" || (a.kind === "install" && a.configure) ? e().configureUrl : null;
  };
  return (
    <li class="fa-cell">
      <div
        class="fa-card"
        data-fgroup={tv() ? undefined : props.fkey}
        // On a TV the card is one focus stop and select performs its action.
        data-f={tv() ? props.fkey : undefined}
        tabindex={tv() ? 0 : undefined}
        role={tv() && pressable() ? "button" : undefined}
        aria-label={tv() ? (pressable() ? label(act()) : `${spoken(e(), { kind: "none", word: word() ?? "" }, props.sameName, provides())}`) : undefined}
        aria-disabled={tv() && busy() ? true : undefined}
        aria-busy={busy() || undefined}
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
          <Show when={failure()}>
            <p class="st bad" role="status">
              <i class="dot" />
              {failure()}
            </p>
          </Show>
        </div>
        <div class="fa-acts">
          <Show when={word()}>
            <span class="fa-state" classList={{ on: act().kind === "none" }}>
              <Show when={act().kind === "none"}>
                <Check aria-hidden />
              </Show>
              {word()}
            </span>
          </Show>
          <Switch>
            <Match when={tv()}>
              <Show when={pressable()}>
                <span class="fa-state pill" classList={{ live: busy() }}>
                  <Show when={busy()} fallback={act().kind === "turnOn" ? <Power aria-hidden /> : <Plus aria-hidden />}>
                    <i class="spin" aria-hidden="true" />
                  </Show>
                  {busy() ? busyWord() : verb()}
                </span>
              </Show>
            </Match>
            <Match when={pressable()}>
              {/* The button stays while its request runs, so keyboard focus stays on it. */}
              <button
                class="btn"
                data-f={`${props.fkey}:act`}
                aria-label={label(act())}
                aria-disabled={busy() || undefined}
                aria-busy={busy() || undefined}
                onClick={perform}
              >
                <Show when={busy()} fallback={act().kind === "turnOn" ? <Power aria-hidden /> : <Plus aria-hidden />}>
                  <i class="spin" aria-hidden="true" />
                </Show>
                {busy() ? busyWord() : verb()}
              </button>
            </Match>
          </Switch>
          <Show when={!tv() && configureUrl()}>
            {(url) => (
              <a class="btn" href={url()} target="_blank" rel="noopener noreferrer" data-f={`${props.fkey}:configure`} aria-label={label({ kind: "configure" })}>
                <ExternalLink aria-hidden />
                {w.configure}
              </a>
            )}
          </Show>
        </div>
      </div>
    </li>
  );
}

function Lists(props: { fresh: boolean; picks: Picks; onPicks: (p: Partial<Picks>) => void; onRetry: () => void }) {
  // Entries are matched by `key`: two entries of a list can share a manifest id.
  const view = liveView<AddonCatalogSnapshot>("addonCatalog", props.fresh ? { fresh: true } : {}, false, "key");
  const tv = () => form() === "tv";
  const lists = () => view.state.data?.lists ?? [];
  const current = () => lists().find((l) => l.id === props.picks.list);
  const ready = (id: string) => lists().find((l) => l.id === id)?.state.state === "ready";
  const count = (id: string) => (ready(id) ? lists().find((l) => l.id === id)?.entries.length.toString() : undefined);
  // Each entry keeps its place in the whole list, so its focus key holds across filters.
  // The query folded once per change; each entry's folded text is kept (lib/find-addons).
  const words = createMemo(() => queryWords(props.picks.query));
  const visible = createMemo(() =>
    (current()?.entries ?? []).map((entry, at) => ({ entry, at })).filter(({ entry }) => shows(entry, props.picks.filter) && matches(entry, words())),
  );
  // The picked list went (its addon was turned off or removed): Official, not "Asking" for ever.
  createEffect(() => {
    if (lists().length > 0 && !current()) props.onPicks({ list: "official", shown: PAGE, query: "" });
  });
  // A publisher's list (an addon the user installed) names where it comes from.
  const host = () => current()?.host ?? null;
  const repeated = createMemo(() => repeatedNames(current()?.entries ?? []));
  const failure = () => {
    const status = view.state.status;
    if (status.state === "failed") return status.reason;
    const s = current()?.state;
    return s?.state === "failed" ? s.words || w.failed : null;
  };
  const more = (el: HTMLElement) => {
    const io = new IntersectionObserver((seen) => seen.some((s) => s.isIntersecting) && props.onPicks({ shown: props.picks.shown + PAGE }), { rootMargin: "1200px 0px" });
    io.observe(el);
    onCleanup(() => io.disconnect());
  };
  // TV: Back from a card goes to the chips; Back from the chips leaves.
  const back = (e: KeyboardEvent) => {
    if (!tv() || !isBackKey(e)) return;
    const chip = document.querySelector<HTMLElement>(`[data-f="fa-lists:${props.picks.list}"]`);
    if (!chip) return;
    e.preventDefault();
    focusEl(chip);
  };
  return (
    <>
      <div class="filters">
        <Chips
          label={w.listLabel}
          group="fa-lists"
          items={lists().map((l) => ({ value: l.id, label: l.name, note: count(l.id) }))}
          selected={props.picks.list}
          onSelect={(list) => props.onPicks({ list, shown: PAGE, query: "" })}
          autofocus
        />
        <Chips label={w.showLabel} group="fa-filter" items={filters} selected={props.picks.filter} onSelect={(filter) => props.onPicks({ filter, shown: PAGE })} />
      </div>
      <Show when={props.picks.list !== "official"}>
        <p class="fa-note">
          <Info aria-hidden />
          {w.community}
        </p>
      </Show>
      <Show when={!tv() && ready(props.picks.list) && (current()?.entries.length ?? 0) > 0}>
        <input
          class="fa-search"
          type="search"
          data-f="fa:search"
          aria-label={w.searchLabel}
          placeholder={w.searchLabel}
          value={props.picks.query}
          onInput={(e) => props.onPicks({ query: e.currentTarget.value, shown: PAGE })}
        />
        <p class="sr" role="status" aria-live="polite">
          {props.picks.query.trim() ? w.matching.replace("{n}", String(visible().length)) : ""}
        </p>
      </Show>
      <div class="fa-tools">
        <Show when={host()}>
          {(from) => (
            <p class="fa-from">
              {/* The core sends hosts of enabled addons on public hosts only: a link there leaves the app on purpose. */}
              <Show when={!tv()} fallback={w.fromList.replace("{host}", from())}>
                <a href={`https://${from()}/`} target="_blank" rel="noopener noreferrer" data-f="fa:from" title={w.fromHint}>
                  {w.fromList.replace("{host}", from())}
                  <ExternalLink aria-hidden />
                </a>
              </Show>
            </p>
          )}
        </Show>
        <Switch>
          <Match when={!failure() && (!current() || current()?.state.state === "loading")}>
            <p class="st warn live" role="status">
              <i class="dot" />
              {host() ? w.loadingFrom.replace("{host}", host() ?? "") : w.loading}
            </p>
          </Match>
          <Match when={ready(props.picks.list)}>
            <button class="btn" data-f="fa:refresh" onClick={props.onRetry}>
              <RotateCw aria-hidden />
              {w.refresh}
            </button>
          </Match>
        </Switch>
      </div>
      <Switch>
        <Match when={failure()}>
          {(words) => (
            <section class="problem" role="alert">
              <p>{words()}</p>
              <div class="acts">
                <button class="btn" data-f="fa:retry" data-autofocus onClick={props.onRetry}>
                  <RotateCw aria-hidden />
                  {w.tryAgain}
                </button>
              </div>
            </section>
          )}
        </Match>
        <Match when={!current() || current()?.state.state === "loading"}>
          <ul class="fa-grid" aria-hidden="true">
            <For each={Array.from({ length: 6 })}>
              {() => (
                <li class="fa-cell">
                  <div class="fa-card skel" />
                </li>
              )}
            </For>
          </ul>
        </Match>
        <Match when={current()?.entries.length === 0 && current()?.more}>
          <p class="note">{w.noRoom}</p>
        </Match>
        <Match when={current()?.entries.length === 0}>
          <p class="note">{host() ? w.emptyFrom.replace("{host}", host() ?? "") : w.empty}</p>
        </Match>
        <Match when={visible().length === 0 && props.picks.query.trim() !== ""}>
          <p class="note">{w.searchEmpty}</p>
        </Match>
        <Match when={visible().length === 0 && props.picks.filter !== "all"}>
          <p class="note">{filterEmpty(props.picks.filter as Exclude<Filter, "all">)}</p>
        </Match>
        <Match when={true}>
          <Show when={current()?.more}>
            <p class="note fa-more">{w.more.replace("{n}", String(current()?.entries.length ?? 0))}</p>
          </Show>
          <ul class="fa-grid" onKeyDown={back}>
            <For each={visible().slice(0, props.picks.shown)}>
              {(item) => <Card entry={item.entry} fkey={`fa:${props.picks.list}:${item.at}`} sameName={repeated().has(item.entry.name)} />}
            </For>
          </ul>
          <Show when={visible().length > props.picks.shown}>
            <div class="sentinel" ref={more} />
          </Show>
        </Match>
      </Switch>
    </>
  );
}

export default function FindAddons() {
  const saved = savedPicks();
  const [picks, setPicks] = createSignal<Picks>(saved);
  const update = (p: Partial<Picks>) => setPicks({ ...picks(), ...p });
  // Kept in the history entry: a list or filter pick at once (a card pressed right after keeps it), typing
  // and scrolling once they settle (Safari refuses more than 100 writes in 30 s).
  let pending: ReturnType<typeof setTimeout> | undefined;
  const keep = (now: Picks) => history.replaceState({ ...(history.state ?? {}), findAddons: now }, "");
  createEffect(on([() => picks().list, () => picks().filter], () => {
    clearTimeout(pending);
    keep(picks());
  }));
  createEffect(on([() => picks().query, () => picks().shown], () => {
    const now = picks();
    clearTimeout(pending);
    pending = setTimeout(() => keep(now), 300);
  }, { defer: true }));
  onCleanup(() => clearTimeout(pending));
  // Try again and Refresh close the view and open it with `fresh`, which
  // waits for the network instead of the addon cache; results from before
  // are about the old list.
  const [opens, setOpens] = createSignal(0);
  const retry = () => {
    forgetAllCards(false);
    setOpens(opens() + 1);
  };
  const tv = () => form() === "tv";
  return (
    <main class="browse page settings-page column find-addons" data-vscroll>
      <h1>{w.title}</h1>
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
        {(n) => <Lists fresh={n > 1} picks={picks()} onPicks={update} onRetry={retry} />}
      </Show>
    </main>
  );
}
