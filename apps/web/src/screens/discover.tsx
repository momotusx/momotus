import { A } from "@solidjs/router";
import Puzzle from "lucide-solid/icons/puzzle";
import Wrench from "lucide-solid/icons/wrench";
import { Match, Show, Switch } from "solid-js";
import { Chips, Grid, SkeletonGrid } from "../components/grid";
import { liveView } from "../core/client";
import type { DiscoverSnapshot } from "../core/generated/DiscoverSnapshot";
import { defaultsWords } from "../lib/defaults";
import "./browse.css";

export default function Discover() {
  const view = liveView<DiscoverSnapshot>("discover");
  const d = () => view.state.data;
  const select = (c: { kind?: string; catalog?: string; genre?: string | null }) => void view.command({ type: "select", ...c });
  const catalogName = () => d()?.catalogs.find((c) => c.id === d()?.catalog)?.name ?? "this catalog";
  const loadMore = () => d()?.state.state === "ready" && void view.command({ type: "loadMore" });

  return (
    <main class="browse page" data-vscroll>
      <h1 data-eyebrow="Browse">Discover</h1>
      <Switch>
        <Match when={view.state.status.state === "failed"}>
          <section class="problem">
            <h2>Discover could not open</h2>
            <p>{(view.state.status as { reason: string }).reason}</p>
          </section>
        </Match>
        <Match when={d() && d()?.kinds.length === 0}>
          <section class="problem">
            <h2>Nothing to browse yet</h2>
            <p>{defaultsWords.emptyDiscover}</p>
            <div class="acts">
              <A class="btn primary" href="/welcome" data-f="discover:setup" data-autofocus>
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
                  items={s().catalogs.map((c) => ({ value: c.id, label: c.name, note: c.source }))}
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
                <Match when={s().items.length === 0 && s().state.state === "loading"}>
                  <SkeletonGrid />
                </Match>
                <Match when={s().items.length === 0 && s().state.state === "failed"}>
                  <p class="note bad">
                    <i class="dot" />
                    {catalogName()} did not answer: {(s().state as { reason: string }).reason}
                  </p>
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
                      <p class="grid-foot st" classList={{ warn: s().state.state === "loading", bad: s().state.state === "failed" }}>
                        <i class="dot" />
                        <Switch fallback={`${s().items.length} titles`}>
                          <Match when={s().state.state === "loading"}>Loading more</Match>
                          <Match when={s().state.state === "end"}>{`All ${s().items.length} titles in ${catalogName()}`}</Match>
                          <Match when={s().state.state === "failed"}>{`Could not load more: ${(s().state as { reason: string }).reason}`}</Match>
                        </Switch>
                      </p>
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
