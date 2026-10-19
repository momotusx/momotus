import ArrowDown from "lucide-solid/icons/arrow-down";
import ArrowUp from "lucide-solid/icons/arrow-up";
import Link2 from "lucide-solid/icons/link-2";
import PackageSearch from "lucide-solid/icons/package-search";
import Plus from "lucide-solid/icons/plus";
import Power from "lucide-solid/icons/power";
import Trash2 from "lucide-solid/icons/trash";
import { A } from "@solidjs/router";
import { createSignal, For, Match, Show, Switch } from "solid-js";
import { liveView, op, reasonOf } from "../core/client";
import type { AddonInfo } from "../core/generated/AddonInfo";
import type { AddonsSnapshot } from "../core/generated/AddonsSnapshot";
import { form } from "../form";
import { findAddons, kindLabel, providesText } from "../lib/words";
import "./browse.css";
import "./settings.css";

/** Installs an addon by URL with a status line under the field. */
/** `onDraft` hears the link as it is typed: a screen with its own primary
 * action gives up the white pill while there is a link to install. */
export function InstallField(props: { autofocus?: boolean; fkey?: string; onDraft?: (url: string) => void }) {
  const [url, setDraft] = createSignal("");
  const setUrl = (v: string) => {
    setDraft(v);
    props.onDraft?.(v);
  };
  const [state, setState] = createSignal<{ tone: string; text: string } | null>(null);
  const install = async (e?: Event) => {
    e?.preventDefault();
    const u = url().trim();
    if (!u) return;
    setState({ tone: "warn live", text: "Installing" });
    try {
      const name = await op<{ id: string; name: string }>("installAddon", { url: u }).then((r) => r.name);
      setState({ tone: "ok", text: `Installed ${name}` });
      setUrl("");
    } catch (err) {
      setState({ tone: "bad", text: `Could not install: ${reasonOf(err)}` });
    }
  };
  const key = props.fkey ?? "install";
  return (
    <form class="install" onSubmit={install}>
      <label class="field-line">
        <span class="kicker">Addon link</span>
        <input
          type="url"
          inputmode="url"
          placeholder="https://example.org/manifest.json or stremio://"
          value={url()}
          onInput={(e) => setUrl(e.currentTarget.value)}
          data-f={`${key}:url`}
          data-autofocus={props.autofocus ? "" : undefined}
          autocomplete="off"
          spellcheck={false}
        />
      </label>
      {/* White only with a link to install: an empty field has no primary action. */}
      <button class="btn" classList={{ primary: !!url().trim() }} type="submit" data-f={`${key}:go`}>
        <Plus aria-hidden />
        Install
      </button>
      <Show when={state()}>
        {(s) => (
          <p class={`st ${s().tone} install-state`} role="status">
            <i class="dot" />
            {s().text}
          </p>
        )}
      </Show>
    </form>
  );
}

function AddonRow(props: { addon: AddonInfo; index: number; count: number }) {
  const a = () => props.addon;
  const [confirm, setConfirm] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  let timer: ReturnType<typeof setTimeout> | undefined;
  const run = (name: string, fields: Record<string, unknown>) =>
    op(name, { url: a().url, ...fields }).catch((e) => setError(reasonOf(e)));
  const remove = () => {
    if (!confirm()) {
      setConfirm(true);
      clearTimeout(timer);
      timer = setTimeout(() => setConfirm(false), 4000);
      return;
    }
    void run("removeAddon", {});
  };
  return (
    <li class="addon" classList={{ off: !a().enabled }} data-fgroup={`addon:${a().host}:${props.index}`}>
      <div class="addon-logo" aria-hidden="true">
        <Show when={a().logo} fallback={<span>{a().name.charAt(0)}</span>}>
          <img src={a().logo as string} alt="" width={64} height={64} loading="lazy" decoding="async" onError={(e) => e.currentTarget.remove()} />
        </Show>
      </div>
      <div class="addon-text">
        <h2>
          {a().name}
          <small class="num">{a().version}</small>
        </h2>
        <p class="host">{a().host}</p>
        <Show when={a().description}>
          <p class="desc">{a().description}</p>
        </Show>
        <p class="provides">
          {a().provides.map(providesText).join(", ")}
          <Show when={a().types.length}>
            <span> for {a().types.map((t) => kindLabel(t).toLowerCase()).join(", ")}</span>
          </Show>
          <Show when={a().configurable}>
            <span>. Settings live on its website.</span>
          </Show>
        </p>
        <Show when={error()}>
          <p class="st bad">
            <i class="dot" />
            {error()}
          </p>
        </Show>
      </div>
      <div class="addon-acts">
        <button
          class="btn"
          aria-pressed={a().enabled}
          data-f={`addon:${props.index}:enabled`}
          data-autofocus={props.index === 0 ? "" : undefined}
          onClick={() => void run("setAddonEnabled", { enabled: !a().enabled })}
        >
          <Power aria-hidden />
          {a().enabled ? "On" : "Off"}
        </button>
        <button class="btn" data-f={`addon:${props.index}:up`} aria-disabled={props.index === 0} onClick={() => props.index > 0 && void run("moveAddon", { to: props.index - 1 })}>
          <ArrowUp aria-hidden />
          Up
        </button>
        <button
          class="btn"
          data-f={`addon:${props.index}:down`}
          aria-disabled={props.index === props.count - 1}
          onClick={() => props.index < props.count - 1 && void run("moveAddon", { to: props.index + 1 })}
        >
          <ArrowDown aria-hidden />
          Down
        </button>
        <button class="btn" classList={{ danger: confirm() }} data-f={`addon:${props.index}:remove`} onClick={remove}>
          <Trash2 aria-hidden />
          {confirm() ? "Press again to remove" : "Remove"}
        </button>
      </div>
    </li>
  );
}

export default function Addons() {
  const view = liveView<AddonsSnapshot>("addons");
  const list = () => view.state.data?.addons ?? [];
  // No typing on a TV by default: the link field waits behind "Paste a
  // link", and focus starts on the first addon.
  const tv = () => form() === "tv";
  const [paste, setPaste] = createSignal(false);
  return (
    <main class="browse page settings-page column" data-vscroll>
      <h1>Addons</h1>
      <p class="lede">Addons bring catalogs, details, streams and subtitles. Order matters: earlier addons answer first.</p>
      <Show
        when={!tv() || paste()}
        fallback={
          <button class="btn disclose" data-f="addons:paste" aria-expanded={false} onClick={() => setPaste(true)}>
            <Link2 aria-hidden />
            Paste a link
          </button>
        }
      >
        <InstallField autofocus={tv()} />
      </Show>
      <A class="btn find-link" href="/find-addons" data-f="addons:find">
        <PackageSearch aria-hidden />
        {findAddons.title}
      </A>
      <Switch>
        <Match when={view.state.status.state === "failed"}>
          <section class="problem">
            <h2>Addons could not be listed</h2>
            <p>{(view.state.status as { reason: string }).reason}</p>
          </section>
        </Match>
        <Match when={view.state.data && list().length === 0}>
          <p class="note">No addons yet. Paste an addon link above, or import them from Stremio in Settings.</p>
        </Match>
        <Match when={true}>
          <ol class="addons">
            <For each={list()}>{(a, i) => <AddonRow addon={a} index={i()} count={list().length} />}</For>
          </ol>
        </Match>
      </Switch>
    </main>
  );
}
