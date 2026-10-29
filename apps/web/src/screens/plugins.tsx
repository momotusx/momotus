/** Settings > Sources > Plugins (plans/plugins-and-marketplace.md 7.7, R7): the profile's Nuvio
 * repositories, each with its providers and a switch per provider, a link field that reads a
 * repository and asks on the install sheet before anything is added, and Remove. */

import Link2 from "lucide-solid/icons/link-2";
import Plus from "lucide-solid/icons/plus";
import Power from "lucide-solid/icons/power";
import Trash2 from "lucide-solid/icons/trash";
import { createResource, createSignal, For, Match, Show, Switch } from "solid-js";
import { InstallSheet } from "../components/install-sheet";
import { op, reasonOf } from "../core/client";
import type { PluginRepoInfo } from "../core/generated/PluginRepoInfo";
import type { PluginRepoPreview } from "../core/generated/PluginRepoPreview";
import type { PluginsAnswer } from "../core/generated/PluginsAnswer";
import { form } from "../form";
import { addsProviders, fill, pluginWords as w } from "../lib/plugins";
import "./browse.css";
import "./settings.css";

/** Reads a pasted link, shows the sheet, and adds the repository only from the sheet's Add. */
function AddField(props: { onAdded: (answer: PluginsAnswer) => void; autofocus?: boolean }) {
  const [url, setUrl] = createSignal("");
  const [state, setState] = createSignal<{ tone: string; text: string } | null>(null);
  const [asking, setAsking] = createSignal<PluginRepoPreview | null>(null);
  let busy = false;
  const check = async (e: Event) => {
    e.preventDefault();
    const u = url().trim();
    if (!u || busy) return;
    busy = true;
    setState({ tone: "warn live", text: w.checking });
    try {
      setAsking(await op<PluginRepoPreview>("previewPluginRepo", { url: u }));
      setState(null);
    } catch (err) {
      setState({ tone: "bad", text: fill(w.couldNotAdd, { reason: reasonOf(err) }) });
    } finally {
      busy = false;
    }
  };
  const add = async (preview: PluginRepoPreview) => {
    setAsking(null);
    setState({ tone: "warn live", text: w.adding });
    try {
      props.onAdded(await op<PluginsAnswer>("addPluginRepo", { url: preview.url }));
      setState({ tone: "ok", text: fill(w.added, { name: preview.name }) });
      setUrl("");
    } catch (err) {
      setState({ tone: "bad", text: fill(w.couldNotAdd, { reason: reasonOf(err) }) });
    }
  };
  return (
    <form class="install" onSubmit={check}>
      <label class="field-line">
        <span class="kicker">{w.fieldLabel}</span>
        <input
          type="url"
          inputmode="url"
          placeholder="https://example.org/manifest.json"
          value={url()}
          onInput={(e) => setUrl(e.currentTarget.value)}
          data-f="plugins:url"
          data-autofocus={props.autofocus ? "" : undefined}
          autocomplete="off"
          spellcheck={false}
        />
      </label>
      <button class="btn" classList={{ primary: !!url().trim() }} type="submit" data-f="plugins:add">
        <Plus aria-hidden />
        {w.add}
      </button>
      <Show when={state()}>
        {(s) => (
          <p class={`st ${s().tone} install-state`} role="status">
            <i class="dot" />
            {s().text}
          </p>
        )}
      </Show>
      <Show when={asking()}>
        {(p) => (
          <InstallSheet
            addon={{ name: p().name, host: p().host, provides: [], types: [] }}
            words={{
              title: fill(w.sheetTitle, { name: p().name }),
              from: fill(w.sheetFrom, { host: p().host }),
              adds: addsProviders(p().providers),
              note: w.sheetNote,
              install: w.add,
            }}
            onCancel={() => setAsking(null)}
            onInstall={() => void add(p())}
          />
        )}
      </Show>
    </form>
  );
}

function RepoRow(props: { repo: PluginRepoInfo; index: number; reload: () => void }) {
  const r = () => props.repo;
  const [confirm, setConfirm] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  let timer: ReturnType<typeof setTimeout> | undefined;
  const run = (name: string, fields: Record<string, unknown>) =>
    op(name, { url: r().url, ...fields })
      .then(props.reload)
      .catch((e) => setError(reasonOf(e)));
  const remove = () => {
    if (!confirm()) {
      setConfirm(true);
      clearTimeout(timer);
      timer = setTimeout(() => setConfirm(false), 4000);
      return;
    }
    void run("removePluginRepo", {});
  };
  return (
    <li class="addon" classList={{ off: !r().enabled }} data-fgroup={`plugin-repo:${props.index}`}>
      <div class="addon-logo" aria-hidden="true">
        <span>{(r().name ?? r().host).charAt(0)}</span>
      </div>
      <div class="addon-text">
        <h2 dir="auto">{r().name ?? r().host}</h2>
        <p class="host">{r().host}</p>
        <Show when={r().description}>
          <p class="desc" dir="auto">
            {r().description}
          </p>
        </Show>
        <Show when={r().problem}>
          {(p) => (
            <p class="st bad">
              <i class="dot" />
              {fill(w.unread, { problem: p() })}
            </p>
          )}
        </Show>
        <Show when={error()}>
          <p class="st bad">
            <i class="dot" />
            {error()}
          </p>
        </Show>
        <Show when={r().providers.length > 0}>
          <ul class="plugin-providers" aria-label={`${w.providers}, ${r().name ?? r().host}`}>
            <For each={r().providers}>
              {(p, i) => (
                <li classList={{ off: !p.enabled || !r().enabled }}>
                  <span class="pname" dir="auto">
                    {p.name}
                    <Show when={p.languages.length}>
                      <small class="num"> {p.languages.join(", ")}</small>
                    </Show>
                  </span>
                  <button
                    class="btn"
                    aria-pressed={p.enabled}
                    aria-label={`${p.name}: ${p.enabled ? w.on : w.off}`}
                    data-f={`plugin:${props.index}:${i()}`}
                    onClick={() => void run("setPluginEnabled", { plugin: p.id, enabled: !p.enabled })}
                  >
                    <Power aria-hidden />
                    {p.enabled ? w.on : w.off}
                  </button>
                </li>
              )}
            </For>
          </ul>
        </Show>
      </div>
      <div class="addon-acts">
        <button
          class="btn"
          aria-pressed={r().enabled}
          data-f={`plugin-repo:${props.index}:enabled`}
          data-autofocus={props.index === 0 ? "" : undefined}
          onClick={() => void run("setPluginRepoEnabled", { enabled: !r().enabled })}
        >
          <Power aria-hidden />
          {r().enabled ? w.on : w.off}
        </button>
        <button class="btn" classList={{ danger: confirm() }} data-f={`plugin-repo:${props.index}:remove`} onClick={remove}>
          <Trash2 aria-hidden />
          {confirm() ? w.removeConfirm : w.remove}
        </button>
      </div>
    </li>
  );
}

export default function Plugins() {
  const [answer, { mutate, refetch }] = createResource(() => op<PluginsAnswer>("plugins"));
  const tv = () => form() === "tv";
  const [paste, setPaste] = createSignal(false);
  const repos = () => answer()?.repos ?? [];
  return (
    <main class="browse page settings-page column" data-vscroll>
      <h1>{w.title}</h1>
      <p class="lede">{w.lede}</p>
      <Switch>
        <Match when={answer.error}>
          <section class="problem">
            <h2>{w.loading}</h2>
            <p>{reasonOf(answer.error)}</p>
          </section>
        </Match>
        <Match when={answer.loading && !answer()}>
          <p class="st warn live" role="status">
            <i class="dot" />
            {w.loading}
          </p>
        </Match>
        <Match when={answer() && !answer()!.available}>
          <p class="note">{w.notAvailable}</p>
        </Match>
        <Match when={answer()}>
          <Show
            when={!tv() || paste()}
            fallback={
              <button class="btn disclose" data-f="plugins:paste" aria-expanded={false} onClick={() => setPaste(true)}>
                <Link2 aria-hidden />
                Paste a link
              </button>
            }
          >
            <AddField autofocus={tv()} onAdded={(a) => mutate(a)} />
          </Show>
          <Show when={repos().length > 0} fallback={<p class="note">{w.empty}</p>}>
            <ol class="addons">
              <For each={repos()}>{(r, i) => <RepoRow repo={r} index={i()} reload={() => void refetch()} />}</For>
            </ol>
          </Show>
        </Match>
      </Switch>
    </main>
  );
}
