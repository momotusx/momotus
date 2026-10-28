// `bun apps/linkhost/build.ts`: the link host's files with the owner's values filled in, into
// apps/linkhost/dist (gitignored), for the Worker in wrangler.jsonc (plans/accounts-social-sync.md 4).
//
// RELAY_URL               the sync service, https://host with no slash at the end (required)
// PROVIDERS               the deletion page's buttons, for example google,facebook,email (required)
// ANDROID_PACKAGES        application ids assetlinks.json names, comma separated (required)
// ANDROID_SIGNING_SHA256  their signing certificate's SHA-256, AA:BB:... (required)
// APPLE_TEAM_ID, APPLE_BUNDLE_ID  both or neither; without them no apple-app-site-association
// APP_STORE_URL, PLAY_STORE_URL   each optional; an unset one drops its button
import { cpSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";

const here = import.meta.dir;
const PAGES = ["pair/index.html", "delete/index.html", "delete/delete.js", "privacy/index.html", "terms/index.html", "linkhost.css", "_headers"];
const KNOWN_PROVIDERS = ["apple", "google", "facebook", "email"];

type Env = Record<string, string | undefined>;

const need = (env: Env, name: string): string => {
  const value = env[name]?.trim();
  if (!value) throw new Error(`${name} is not set`);
  return value;
};

/** Every line holding `{{NAME}}` goes when the value is unset; otherwise the placeholder is filled. */
const fillOrDrop = (text: string, name: string, value: string | undefined): string =>
  value ? text.replaceAll(`{{${name}}}`, value) : text.split("\n").filter((line) => !line.includes(`{{${name}}}`)).join("\n");

/** The deletion page with only the buttons of the providers the sync service offers. */
const deletePage = (html: string, providers: string[]): string =>
  html
    .split("\n")
    .filter((line) => !line.includes("One button per provider"))
    .filter((line) => {
      const button = line.match(/data-provider="([a-z]+)"/);
      if (button) return providers.includes(button[1]);
      return !(line.includes('id="apple-note"') && !providers.includes("apple"));
    })
    .join("\n");

const assetLinks = (packages: string[], sha256: string): string =>
  JSON.stringify(
    packages.map((name) => ({
      relation: ["delegate_permission/common.handle_all_urls", "delegate_permission/common.get_login_creds"],
      target: { namespace: "android_app", package_name: name, sha256_cert_fingerprints: [sha256] },
    })),
    null,
    2,
  ) + "\n";

const filesUnder = (dir: string): string[] =>
  readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    return statSync(path).isDirectory() ? filesUnder(path) : [path];
  });

/** Builds from `from` (this folder) into `out` and answers the files written, relative to it. */
export function build(env: Env, out: string, from = here): string[] {
  const relay = need(env, "RELAY_URL");
  if (!/^https:\/\/[a-z0-9.-]+$/.test(relay)) throw new Error("RELAY_URL must be https://host, lowercase, no port, path or slash");
  const providers = need(env, "PROVIDERS").split(",").map((p) => p.trim());
  const unknown = providers.filter((p) => !KNOWN_PROVIDERS.includes(p));
  if (unknown.length) throw new Error(`PROVIDERS names unknown providers: ${unknown.join(", ")}`);
  const packages = need(env, "ANDROID_PACKAGES").split(",").map((p) => p.trim());
  const sha256 = need(env, "ANDROID_SIGNING_SHA256");
  if (!/^([0-9A-F]{2}:){31}[0-9A-F]{2}$/.test(sha256)) throw new Error("ANDROID_SIGNING_SHA256 must be 32 bytes as AA:BB:...");
  const team = env.APPLE_TEAM_ID?.trim();
  const bundle = env.APPLE_BUNDLE_ID?.trim();
  if (!team !== !bundle) throw new Error("set APPLE_TEAM_ID and APPLE_BUNDLE_ID together, or neither");

  rmSync(out, { recursive: true, force: true });
  for (const page of PAGES) {
    let text = readFileSync(join(from, page), "utf8").replaceAll("{{RELAY_URL}}", relay);
    text = fillOrDrop(text, "APP_STORE_URL", env.APP_STORE_URL?.trim());
    text = fillOrDrop(text, "PLAY_STORE_URL", env.PLAY_STORE_URL?.trim());
    if (page === "delete/index.html") text = deletePage(text, providers);
    mkdirSync(dirname(join(out, page)), { recursive: true });
    writeFileSync(join(out, page), text);
  }
  mkdirSync(join(out, ".well-known"), { recursive: true });
  writeFileSync(join(out, ".well-known/assetlinks.json"), assetLinks(packages, sha256));
  if (team && bundle) {
    cpSync(join(from, ".well-known/apple-app-site-association"), join(out, ".well-known/apple-app-site-association"));
    const aasa = join(out, ".well-known/apple-app-site-association");
    writeFileSync(aasa, readFileSync(aasa, "utf8").replaceAll("{{APPLE_TEAM_ID}}", team).replaceAll("{{APPLE_BUNDLE_ID}}", bundle));
  } else {
    // The headers file names the association file's type; without the file, its rule goes too.
    const headers = join(out, "_headers");
    writeFileSync(headers, readFileSync(headers, "utf8").replace(/\n\/\.well-known\/apple-app-site-association\n[^\n]*\n?/, "\n"));
  }

  const written = filesUnder(out).map((path) => relative(out, path)).sort();
  const left = written.filter((file) => readFileSync(join(out, file), "utf8").includes("{{"));
  if (left.length) throw new Error(`placeholders left in ${left.join(", ")}`);
  return written;
}

if (import.meta.main) {
  try {
    for (const file of build(process.env, join(here, "dist"))) console.log(file);
  } catch (e) {
    console.error(`linkhost build: ${(e as Error).message}`);
    process.exit(1);
  }
}
