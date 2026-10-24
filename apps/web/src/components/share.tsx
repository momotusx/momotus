/** Share for a title or an episode (plans/appletv-polish.md decision 11): its name and its IMDb page,
 * through the system's share sheet where the browser has one, else copied, the word saying so for a
 * moment. design-web places it last in the title's action row; never on TV. */
import Check from "lucide-solid/icons/check";
import Share2 from "lucide-solid/icons/share-2";
import { createSignal, onCleanup, Show } from "solid-js";

/** What goes out: the title as the viewer reads it, then the link. */
export const shareText = (name: string, url: string) => `${name} ${url}`;

export function ShareButton(props: { name: string; url: string }) {
  const [copied, setCopied] = createSignal(false);
  let timer: ReturnType<typeof setTimeout> | undefined;
  onCleanup(() => clearTimeout(timer));
  const share = async () => {
    const data = { title: props.name, text: props.name, url: props.url };
    if (navigator.share && navigator.canShare?.(data) !== false) {
      // A share the viewer closed is no failure.
      await navigator.share(data).catch(() => undefined);
      return;
    }
    try {
      await navigator.clipboard.writeText(shareText(props.name, props.url));
      setCopied(true);
      clearTimeout(timer);
      timer = setTimeout(() => setCopied(false), 2000);
    } catch {
      // No clipboard either (an insecure page): nothing is claimed.
    }
  };
  return (
    <button class="rbtn" data-f="title:share" aria-label={`Share ${props.name}`} onClick={() => void share()}>
      <span class="ring">
        <Show when={copied()} fallback={<Share2 aria-hidden />}>
          <Check aria-hidden />
        </Show>
      </span>
      <span class="lbl" role="status">
        {copied() ? "Link copied" : "Share"}
      </span>
    </button>
  );
}
