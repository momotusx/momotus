import { A } from "@solidjs/router";
import Compass from "lucide-solid/icons/compass";
import { Match, Switch } from "solid-js";
import { Chips, Grid, SkeletonGrid } from "../components/grid";
import { liveView } from "../core/client";
import type { LibrarySnapshot } from "../core/generated/LibrarySnapshot";
import type { LibrarySort } from "../core/generated/LibrarySort";
import "./browse.css";

const sorts: { value: LibrarySort; label: string }[] = [
  { value: "recent", label: "Last watched" },
  { value: "added", label: "Recently added" },
  { value: "name", label: "Name" },
];

export default function Library() {
  const view = liveView<LibrarySnapshot>("library");
  const l = () => view.state.data;
  const total = () => l()?.kinds.reduce((n, k) => n + k.count, 0) ?? 0;

  return (
    <main class="browse page" data-vscroll>
      <h1 data-eyebrow="Yours">Library</h1>
      <Switch fallback={<SkeletonGrid />}>
        <Match when={view.state.status.state === "failed"}>
          <section class="problem">
            <h2>Your library could not open</h2>
            <p>{(view.state.status as { reason: string }).reason}</p>
          </section>
        </Match>
        <Match when={l() && total() === 0}>
          <section class="problem">
            <h2>Your library is empty</h2>
            <p>Add titles with Add to Library on a title page, or import your Stremio library from Settings.</p>
            <A class="btn primary" href="/discover" data-f="library:discover" data-autofocus>
              <Compass aria-hidden />
              Discover
            </A>
          </section>
        </Match>
        <Match when={l()}>
          {(s) => (
            <>
              <div class="filters">
                <Chips
                  label="Show"
                  group="lib-kinds"
                  items={[{ value: null as string | null, label: "All", note: String(total()) }, ...s().kinds.map((k) => ({ value: k.kind as string | null, label: k.label, note: String(k.count) }))]}
                  selected={s().kind}
                  onSelect={(kind) => void view.command({ type: "setFilter", kind })}
                  autofocus
                />
                <Chips label="Sort" group="lib-sort" items={sorts} selected={s().sort} onSelect={(sort) => void view.command({ type: "setSort", sort })} />
              </div>
              <Grid items={s().items} scope="library" />
            </>
          )}
        </Match>
      </Switch>
    </main>
  );
}
