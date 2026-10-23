import { A, useSearchParams } from "@solidjs/router";
import Puzzle from "lucide-solid/icons/puzzle";
import RefreshCw from "lucide-solid/icons/refresh-cw";
import Wrench from "lucide-solid/icons/wrench";
import { createEffect, createSignal, Match, on, Show, Switch } from "solid-js";
import { Chips, Grid, SkeletonGrid } from "../components/grid";
import { Why } from "../components/why";
import { discoverView } from "../core/client";
import { onProfileReset } from "../core/profiles";
import { defaultsWords } from "../lib/defaults";
import { type DiscoverLink, discoverLinkStep } from "../lib/discover-link";
import { problemWords } from "../lib/words";
import "./browse.css";

// Where the page was scrolled to: the view is kept, so Back lands here too.
const pageScroll = { top: 0 };
onProfileReset(() => {
  pageScroll.top = 0;
});

export default function Discover() {
  // Try again on Discover that could not open: a new view, as the page anew.
  const [attempt, setAttempt] = createSignal(0);
  return (
    <Show when={attempt() + 1} keyed>
      <DiscoverPage retry={() => setAttempt((n) => n + 1)} />
    </Show>
  );
}

function DiscoverPage(props: { retry: () => void }) {
  const view = discoverView();
  const attach = (el: HTMLElement) => {
    if (pageScroll.top) queueMicrotask(() => el.scrollTo({ top: pageScroll.top }));
  };
  const d = () => view.state.data;
  const select = (c: { kind?: string; catalog?: string; genre?: string | null }) => void view.command({ type: "select", ...c });
  const catalogName = () => d()?.catalogs.find((c) => c.id === d()?.catalog)?.name ?? "this catalog";
  const loadMore = () => d()?.state.state === "ready" && void view.command({ type: "loadMore" });
  // A link from a title's genre chip (`?kind=series&genre=Drama`) opens Discover at its top on that kind
  // and genre. It is taken from the URL once and dropped with a replace, so Back to Discover, or the
  // viewer's own next choice, never has it applied again.
  const [params, setParams] = useSearchParams();
  if (params.kind || params.genre) pageScroll.top = 0;
  const [link, setLink] = createSignal<DiscoverLink | null>(null);
  createEffect(() => {
    const kind = typeof params.kind === "string" ? params.kind : undefined;
    const genre = typeof params.genre === "string" ? params.genre : undefined;
    if (!kind && !genre) return;
    setLink({ kind, genre });
    setParams({ kind: undefined, genre: undefined }, { replace: true });
  });
  let sent = "";
  createEffect(() => {
    const l = link();
    const s = d();
    if (!l || !s) return;
    const step = discoverLinkStep(l, s);
    if (step === "wait") return;
    if (step === "done") return void setLink(null);
    // One command per step: the snapshot that answers it comes later.
    const key = JSON.stringify(step);
    if (key === sent) return;
    sent = key;
    select(step);
  });
  // Try again after a failed page (or a failed catalog): loadMore asks that
  // page again. Its button stays, reading "Trying again", until the answer,
  // so the remote keeps its place.
  const [retrying, setRetrying] = createSignal(false);
  createEffect(on(() => d()?.state.state, (st) => st !== "loading" && setRetrying(false), { defer: true }));
  const retry = () => {
    if (retrying()) return;
    setRetrying(true);
    view.command({ type: "loadMore" }).catch(() => setRetrying(false));
  };
  const retryButton = (key: string) => (
    <button class="btn" data-f={key} aria-disabled={retrying()} onClick={retry}>
      <RefreshCw aria-hidden />
      {retrying() ? "Trying again" : "Try again"}
    </button>
  );

  return (
    <main class="browse page" data-vscroll ref={attach} onScroll={(e) => (pageScroll.top = e.currentTarget.scrollTop)}>
      <h1>Discover</h1>
      <Switch>
        <Match when={view.state.status.state === "failed"}>
          <section class="problem">
            <h2>Discover could not open</h2>
            <Why reason={(view.state.status as { reason: string }).reason} />
            <button class="btn primary" data-f="discover:retry" data-autofocus onClick={() => props.retry()}>
              <RefreshCw aria-hidden />
              Try again
            </button>
          </section>
        </Match>
        <Match when={d() && d()?.kinds.length === 0}>
          <section class="problem">
            <h2>Nothing to browse yet</h2>
            <p>{defaultsWords.emptyDiscover}</p>
            <div class="acts">
              {/* The recommended addons to switch on, not the first-run doors. */}
              <A class="btn primary" href="/welcome?step=2&review=1" data-f="discover:setup" data-autofocus>
                <Wrench aria-hidden />
                Set up Marquee
              </A>
              <A class="btn" href="/addons" data-f="discover:addons">
                <Puzzle aria-hidden />
                Add an addon
              </A>
            </div>
          </section>
        </Match>
        <Match when={d()}>
          {(s) => (
            <>
              <div class="filters">
                <Chips
                  label="Type"
                  group="kinds"
                  items={s().kinds.map(([value, label]) => ({ value, label }))}
                  selected={s().kind}
                  onSelect={(kind) => select({ kind: kind ?? undefined })}
                  autofocus
                />
                <Chips
                  label="Catalog"
                  group="catalogs"
                  items={s().catalogs.map((c) => ({ value: c.id, label: c.name }))}
                  selected={s().catalog}
                  onSelect={(catalog) => select({ catalog: catalog ?? undefined })}
                />
                <Show when={s().genres.length}>
                  <Chips
                    label="Genre"
                    group="genres"
                    items={[...(s().genreRequired ? [] : [{ value: null as string | null, label: "All" }]), ...s().genres.map((g) => ({ value: g as string | null, label: g }))]}
                    selected={s().genre}
                    onSelect={(genre) => select({ genre })}
                  />
                </Show>
              </div>
              <Switch>
                <Match when={s().genreRequired && !s().genre}>
                  <p class="note">{catalogName()} only answers for one genre at a time. Pick a genre above.</p>
                </Match>
                <Match when={s().items.length === 0 && (s().state.state === "failed" || retrying())}>
                  <div class="discover-fail">
                    <p class="note" classList={{ bad: !retrying() }}>
                      <i class="dot" />
                      {retrying()
                        ? `Asking for ${catalogName()} again.`
                        : `${catalogName()} could not load. ${problemWords((s().state as { reason: string }).reason).text}`}
                    </p>
                    {retryButton("discover:retry-catalog")}
                  </div>
                </Match>
                <Match when={s().items.length === 0 && s().state.state === "loading"}>
                  <SkeletonGrid />
                </Match>
                <Match when={s().items.length === 0}>
                  <p class="note">Nothing in {catalogName()} for this choice.</p>
                </Match>
                <Match when={true}>
                  <Grid
                    items={s().items}
                    scope="discover"
                    onNearEnd={loadMore}
                    footer={
                      <div class="grid-foot-row">
                        <p class="grid-foot st" classList={{ warn: s().state.state === "loading", bad: s().state.state === "failed" }}>
                          <i class="dot" />
                          <Switch fallback={`${s().items.length} titles`}>
                            <Match when={s().state.state === "loading"}>Loading more</Match>
                            <Match when={s().state.state === "end"}>{`All ${s().items.length} titles in ${catalogName()}`}</Match>
                            <Match when={s().state.state === "failed"}>{`Could not load more. ${problemWords((s().state as { reason: string }).reason).text}`}</Match>
                          </Switch>
                        </p>
                        <Show when={s().state.state === "failed" || retrying()}>{retryButton("discover:retry-more")}</Show>
                      </div>
                    }
                  />
                </Match>
              </Switch>
            </>
          )}
        </Match>
        <Match when={true}>
          <SkeletonGrid />
        </Match>
      </Switch>
    </main>
  );
}
