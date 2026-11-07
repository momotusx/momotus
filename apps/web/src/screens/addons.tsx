import ArrowDown from "lucide-solid/icons/arrow-down";
import ArrowUp from "lucide-solid/icons/arrow-up";
import Link2 from "lucide-solid/icons/link-2";
import PackageSearch from "lucide-solid/icons/package-search";
import Plus from "lucide-solid/icons/plus";
import Power from "lucide-solid/icons/power";
import Trash2 from "lucide-solid/icons/trash";
import { A } from "@solidjs/router";
import { createSignal, For, Match, onCleanup, Show, Switch } from "solid-js";
import { isTokenInLink, liveView, op, reasonOf } from "../core/client";
import type { AddonInfo } from "../core/generated/AddonInfo";
import { InstallSheet } from "../components/install-sheet";
import type { AddonsSnapshot } from "../core/generated/AddonsSnapshot";
import { form } from "../form";
import { autoFocus } from "../focus/dom";
import { sheetWords } from "../lib/install-sheet";
import { addonHost, findAddons, kindLabel, providesText, tokenInLink } from "../lib/words";
import "./browse.css";
import "./settings.css";
import { incognito } from "../core/profiles";
import { incognitoWords } from "../components/incognito";

/** Installs an addon by URL with a status line under the field: the link is read first and the
 * install sheet shows what it is (plans/addon-install-sheet.md); only its Install installs. */
/** `onDraft` hears the link as it is typed: a screen with its own primary
 * action gives up the white pill while there is a link to install. */
export function InstallField(props: { autofocus?: boolean; fkey?: string; onDraft?: (url: string) => void }) {
  const [url, setDraft] = createSignal("");
  const setUrl = (v: string) => {
    setDraft(v);
    props.onDraft?.(v);
  };
  const [state, setState] = createSignal<{ tone: string; text: string } | null>(null);
  // A link the core refused because it carries a sign-in, waiting for "It is mine".
  const [risky, setRisky] = createSignal<string | null>(null);
  // The link read and waiting on the sheet, with whether its sign-in was acknowledged.
  const [asking, setAsking] = createSignal<{ info: AddonInfo; url: string; acknowledged: boolean } | null>(null);
  let checking = false;
  const fields = (u: string, acknowledged: boolean) => (acknowledged ? { url: u, acknowledgeToken: true } : { url: u });
  const check = async (e?: Event, acknowledged = false) => {
    e?.preventDefault();
    const u = acknowledged ? (risky() ?? "") : url().trim();
    if (!u || checking) return;
    checking = true;
    // "It is mine" goes while the link is read: focus moves to Install first, so the sheet gives it
    // back there when it closes.
    if (acknowledged) document.querySelector<HTMLElement>(`[data-f="${key}:go"]`)?.focus();
    setRisky(null);
    setState({ tone: "warn live", text: sheetWords.checking });
    try {
      const info = await op<AddonInfo>("previewAddon", fields(u, acknowledged));
      setState(null);
      setAsking({ info, url: u, acknowledged });
    } catch (err) {
      if (isTokenInLink(err)) {
        setRisky(u);
        setState({ tone: "warn", text: tokenInLink.warn });
        return;
      }
      setState({ tone: "bad", text: `Could not install: ${reasonOf(err)}` });
    } finally {
      checking = false;
    }
  };
  const install = async (asked: { url: string; acknowledged: boolean }) => {
    setAsking(null);
    setState({ tone: "warn live", text: "Installing" });
    try {
      const name = await op<{ id: string; name: string }>("installAddon", fields(asked.url, asked.acknowledged)).then((r) => r.name);
      setState({ tone: "ok", text: `Installed ${name}` });
      setUrl("");
    } catch (err) {
      setState({ tone: "bad", text: `Could not install: ${reasonOf(err)}` });
    }
  };
  const key = props.fkey ?? "install";
  return (
    <form class="install" onSubmit={check}>
      <label class="field-line">
        <span class="kicker">Addon link</span>
        <input
          type="url"
          inputmode="url"
          placeholder="https://example.org/manifest.json or stremio://"
          value={url()}
          onInput={(e) => {
            setUrl(e.currentTarget.value);
            if (risky()) {
              setRisky(null);
              setState(null);
            }
          }}
          data-f={`${key}:url`}
          data-autofocus={props.autofocus ? "" : undefined}
          autocomplete="off"
          spellcheck={false}
        />
      </label>
      {/* White only with a link to install: an empty field has no primary action. */}
      <button class="btn" classList={{ primary: !!url().trim() && !risky() }} type="submit" data-f={`${key}:go`}>
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
      <Show when={risky()}>
        {/* A risky choice, not the screen's primary: glass with the warning's edge. */}
        <button class="btn caution" type="button" data-f={`${key}:ack`} onClick={() => void check(undefined, true)}>
          {tokenInLink.confirm}
        </button>
      </Show>
      <Show when={asking()}>
        {(asked) => <InstallSheet addon={asked().info} onCancel={() => setAsking(null)} onInstall={() => void install(asked())} />}
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
  // Up or Down: the rows keep their places and show the list in its new
  // order, so the remote follows the addon to the same button on its new row.
  const move = (to: number, button: "up" | "down") =>
    op("moveAddon", { url: a().url, to })
      .then(() => autoFocus(document.querySelector<HTMLElement>(`[data-f="addon:${to}:${button}"]`)))
      .catch((e) => setError(reasonOf(e)));
  let removing = false;
  const remove = () => {
    if (!confirm()) {
      setConfirm(true);
      clearTimeout(timer);
      timer = setTimeout(() => setConfirm(false), 4000);
      return;
    }
    removing = true;
    void run("removeAddon", {});
  };
  // Removed by its own Remove: the remote stays where the row was, on the next row's Remove, or the one above
  // when it was the last, never back on the first row (QA r1 Q6 row 51).
  onCleanup(() => {
    if (!removing) return;
    const at = props.index;
    const button = (i: number) => document.querySelector<HTMLElement>(`[data-f="addon:${i}:remove"]`);
    requestAnimationFrame(() => autoFocus(button(at) ?? button(at - 1)));
  });
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
        {/* The row names the addon by its own name and logo; its host only behind Details. */}
        <Show
          when={!a().builtIn}
          fallback={<p class="host">{addonHost(a())}</p>}
        >
          <details class="why-raw addon-host">
            <summary data-f={`addon:${props.index}:details`}>Details</summary>
            <code class="host">{a().host}</code>
          </details>
        </Show>
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
        <button class="btn" data-f={`addon:${props.index}:up`} aria-disabled={props.index === 0} onClick={() => props.index > 0 && void move(props.index - 1, "up")}>
          <ArrowUp aria-hidden />
          Up
        </button>
        <button
          class="btn"
          data-f={`addon:${props.index}:down`}
          aria-disabled={props.index === props.count - 1}
          onClick={() => props.index < props.count - 1 && void move(props.index + 1, "down")}
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
      {/* Incognito works on its own copy of the addons (plans/batch-10-incognito.md 3). */}
      <Show when={incognito()}>
        <p class="note">{incognitoWords.addonsNote}</p>
      </Show>
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
