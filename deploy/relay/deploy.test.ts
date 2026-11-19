// `bun test deploy/relay`: render.sh's checks, and deploy.sh's remote half run here with every
// system command stubbed (it logs its arguments), to see what the server would be asked to run.
import { expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import { chmodSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const HERE = import.meta.dir;
const REMOTE = readFileSync(join(HERE, "deploy.sh"), "utf8").match(/<<'REMOTE'\n([\s\S]*?)\nREMOTE\n/)![1];
const STUBS = ["systemctl", "systemd-run", "install", "runuser", "journalctl", "ln", "readlink", "ls", "xargs", "rm"];

function render(instance: string) {
  const out = mkdtempSync(join(tmpdir(), "relay-stage-"));
  const run = spawnSync("bash", [join(HERE, "render.sh"), instance, out], { encoding: "utf8" });
  return { status: run.status, stderr: run.stderr, out };
}

function instance(values: string) {
  const path = join(mkdtempSync(join(tmpdir(), "relay-instance-")), "other.env");
  writeFileSync(path, values);
  return path;
}

/** The remote half on a rendered stage; returns each stubbed call's arguments. */
function remote(stage: string) {
  const bin = mkdtempSync(join(tmpdir(), "relay-bin-"));
  const log = join(bin, "calls.log");
  const stub = (name: string, body = "") =>
    writeFileSync(join(bin, name), `#!/bin/bash\nprintf '%s\\0' "${name}" "$@" >>"${log}"; printf '\\n' >>"${log}"\n${body}\n`);
  for (const name of STUBS) stub(name);
  stub("sha256sum", 'echo "feed  $1"');
  // Health answers; a pairing with a client address is taken, one without refused.
  stub("curl", 'case "$*" in *cf-connecting-ip*) echo 200 ;; *v1/pairings*) echo 400 ;; esac');
  for (const name of [...STUBS, "sha256sum", "curl"]) chmodSync(join(bin, name), 0o755);
  const run = spawnSync("bash", ["-s", "--", stage, "c0ffee11", "feed"], {
    input: REMOTE,
    env: { PATH: `${bin}:/usr/bin:/bin` },
    encoding: "utf8",
  });
  const calls = readFileSync(log, "utf8")
    .split("\n")
    .filter(Boolean)
    .map((line) => line.split("\0").filter(Boolean));
  return { status: run.status, stderr: run.stderr, calls };
}

test("Momotus's preflight gets its flags and credentials whole", () => {
  const { status, out } = render(join(HERE, "momotus.env"));
  expect(status).toBe(0);
  const run = remote(out);
  expect(run.stderr).toBe("");
  expect(run.status).toBe(0);
  const preflight = run.calls.find((c) => c[0] === "systemd-run")!;
  expect(preflight).toContain("LoadCredential=identity.json:/etc/momotus/identity.json");
  expect(preflight).toContain("LoadCredential=ses.json:/etc/momotus/ses.json");
  const command = preflight[preflight.length - 1];
  expect(command).toContain("--listen 127.0.0.1:8789");
  expect(command).toContain('--mail "$CREDENTIALS_DIRECTORY"/ses.json --reports /var/lib/momotus-preflight/reports.sqlite $RELAY_FLAGS');
  expect(run.calls.some((c) => c[0] === "systemctl" && c.includes("momotus-relay"))).toBe(true);
});

test("an instance with no extra credentials deploys its own flags", () => {
  const values = 'NAME=lus\nTITLE="Lus relay"\nHOST=sync.lus.example\nPORT=8797\nPREFLIGHT_PORT=8799\nFLAGS="--escrow --quota-mib 200"\nCREDENTIALS=""\n';
  const { status, out } = render(instance(values));
  expect(status).toBe(0);
  const unit = readFileSync(join(out, "lus-relay.service"), "utf8");
  expect(unit).toContain("--listen 127.0.0.1:8797 --trust-proxy --public-host sync.lus.example --identity %d/identity.json --escrow --quota-mib 200 $RELAY_FLAGS");
  expect(unit).not.toContain("ses.json:");
  expect(unit.split("\n")[1]).toContain("@NAME@"); // comments are not rendered
  const run = remote(out);
  expect(run.status).toBe(0);
  const preflight = run.calls.find((c) => c[0] === "systemd-run")!;
  expect(preflight.filter((a) => a.startsWith("LoadCredential="))).toEqual(["LoadCredential=identity.json:/etc/lus/identity.json"]);
  expect(preflight[preflight.length - 1]).toContain("--escrow --quota-mib 200 $RELAY_FLAGS");
});

test("values a root shell would run, or a unit would read as directives, are refused", () => {
  const base = 'NAME=lus\nTITLE="Lus relay"\nHOST=sync.lus.example\nPORT=8797\nPREFLIGHT_PORT=8799\n';
  for (const bad of [
    "FLAGS='--escrow $(touch /tmp/x)'",
    "FLAGS='--escrow; reboot'",
    'HOST="a.example; reboot"',
    "PORT=87x7",
    'TITLE="x\nExecStartPre=/bin/false"',
    'CREDENTIALS="../shadow"',
  ]) {
    const run = render(instance(`${base}${bad}\n`));
    expect(run.status).toBe(1);
    expect(run.stderr).toContain("must match");
  }
});
