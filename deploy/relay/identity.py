"""The relay's identity file on the server, /etc/<NAME>/identity.json (plans/accounts-social-sync.md H5, phase E).

    sudo python3 -I identity.py create [NAME HOST] [--escrow]
                                     a key-only file: a new subject key (and with --escrow an escrow
                                     key, for a relay run with --escrow), never printed
    sudo python3 -I identity.py merge [NAME] < env-file
                                     the provider blocks from KEY=VALUE lines on stdin, the keys,
                                     publicUrl and linkHost kept
    sudo python3 -I identity.py escrow-key [NAME]
                                     identity.json.next: the file with a new escrow key, for
                                     `mq-sync-server rekey --to` (README.md, Rekey)
    python3 -I identity.py --self-test   this file's own rules, on a scratch folder

NAME is the relay instance (momotus.env's NAME; momotus when none is given), HOST its public host.

`merge` refuses a file without a key (a new key would turn every linked identity into a new one,
and the relay refuses to start on another key's fingerprint), keeps the previous file as
identity.json.prev, and replaces the file atomically. Each prints only key fingerprints.
Standard library only.
"""

from __future__ import annotations

import hashlib
import json
import os
import secrets
import sys
import tempfile
from pathlib import Path

NAME = "momotus"
HOST = "momotus.normalisdead.mom"


def path_of(name: str) -> Path:
    return Path("/etc") / name / "identity.json"
GRAPH = "v26.0"


def fingerprint(key: str) -> str:
    return hashlib.sha256(bytes.fromhex(key)).hexdigest()[:16]


def escrow_fingerprint(key: str) -> str:
    """The relay's own name of an escrow key, as its database keeps it (meta escrow_key)."""
    return hashlib.sha256(b"marquee/escrow-key/v1\0" + bytes.fromhex(key)).hexdigest()[:32]


def write(path: Path, data: dict) -> None:
    tmp = path.with_name(f".{path.name}.tmp")
    fd = os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(fd, "w") as f:
        json.dump(data, f, indent=2)
        f.write("\n")
    os.replace(tmp, path)


def create(path: Path, host: str = HOST, escrow: bool = False) -> str:
    if path.exists():
        raise SystemExit(f"{path} exists; merge into it instead")
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    key = secrets.token_hex(32)
    data = {"key": key, "publicUrl": f"https://{host}", "linkHost": host}
    if escrow:
        data["escrowKey"] = secrets.token_hex(32)
    write(path, data)
    return fingerprint(key)


def escrow_key(path: Path) -> str:
    """Writes identity.json.next: the file as it is, with a new escrow key."""
    current = json.loads(path.read_text())
    if not current.get("escrowKey"):
        raise SystemExit(f"{path} has no escrowKey: this relay keeps no escrow")
    new = secrets.token_hex(32)
    write(path.with_name(f"{path.name}.next"), current | {"escrowKey": new})
    return escrow_fingerprint(new)


def providers(env_text: str) -> dict:
    env = dict(line.split("=", 1) for line in env_text.splitlines() if "=" in line and not line.lstrip().startswith("#"))
    env = {k.strip(): v.strip().strip('"').strip("'") for k, v in env.items()}
    blocks: dict = {}
    if env.get("GOOGLE_WEB_CLIENT_ID"):
        blocks["google"] = {"webClientId": env["GOOGLE_WEB_CLIENT_ID"], "webClientSecret": env["GOOGLE_WEB_CLIENT_SECRET"]}
    if env.get("FACEBOOK_APP_ID"):
        # The relay signs in with PKCE: the app secret is optional.
        blocks["facebook"] = {
            "appId": env["FACEBOOK_APP_ID"],
            "graph": f"https://graph.facebook.com/{GRAPH}",
            "dialog": f"https://www.facebook.com/{GRAPH}/dialog/oauth",
        }
        if env.get("FACEBOOK_APP_SECRET"):
            blocks["facebook"]["appSecret"] = env["FACEBOOK_APP_SECRET"]
    if not blocks:
        raise SystemExit("no provider values on stdin")
    return blocks


def merge(path: Path, env_text: str) -> str:
    current = json.loads(path.read_text())
    key = current.get("key")
    if not key:
        raise SystemExit(f"{path} has no key: refusing to make a new one")
    before = fingerprint(key)
    # Every other entry (an Apple or OpenID provider set by hand) stays; the
    # file's Google and Facebook blocks are the ones the env file names.
    merged = {k: v for k, v in current.items() if k not in ("google", "facebook")} | providers(env_text)
    write(path.with_name(f"{path.name}.prev"), current)
    write(path, merged)
    after = fingerprint(json.loads(path.read_text())["key"])
    assert after == before, "the key changed"
    return after


def self_test() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "momotus" / "identity.json"
        first = create(path)
        assert oct(path.stat().st_mode & 0o777) == "0o600"
        assert json.loads(path.read_text())["linkHost"] == HOST
        current = json.loads(path.read_text())
        write(path, current | {"oidc": [{"id": "x"}]})
        env = "GOOGLE_WEB_CLIENT_ID=g.apps\nGOOGLE_WEB_CLIENT_SECRET=\"gs\"\n# a comment\nFACEBOOK_APP_ID=1\nFACEBOOK_APP_SECRET='fs'\n"
        assert merge(path, env) == first
        data = json.loads(path.read_text())
        assert data["google"] == {"webClientId": "g.apps", "webClientSecret": "gs"}
        assert data["facebook"]["graph"].endswith("/v26.0") and data["facebook"]["appSecret"] == "fs"
        assert json.loads(path.with_name("identity.json.prev").read_text()).get("google") is None
        assert data["oidc"] == [{"id": "x"}], "entries set by hand stay"
        # A second merge replaces the providers and still keeps the key.
        assert merge(path, "FACEBOOK_APP_ID=2\nFACEBOOK_APP_SECRET=fs2\n") == first
        assert "google" not in json.loads(path.read_text())
        # Facebook with no secret (the relay's PKCE flow): no appSecret entry.
        assert merge(path, "FACEBOOK_APP_ID=3\n") == first
        assert "appSecret" not in json.loads(path.read_text())["facebook"]
        # No key, no merge; an existing file is never created over.
        write(path, {"publicUrl": "https://x", "linkHost": "x"})
        for action in (lambda: merge(path, env), lambda: create(path)):
            try:
                action()
            except SystemExit:
                pass
            else:
                raise AssertionError("a file without a key, or an existing file, was written")
        # Another instance, with escrow: its own host; a new escrow key goes to .next only.
        other = Path(tmp) / "other" / "identity.json"
        create(other, "other.example", escrow=True)
        made = json.loads(other.read_text())
        # The relay's form (mq-sync escrow.rs pins the same value).
        assert escrow_fingerprint("07" * 32) == "91c252dc3ff49e79e90146f3cd754b40"
        assert made["linkHost"] == "other.example" and len(made["escrowKey"]) == 64
        escrow_key(other)
        nxt = json.loads(other.with_name("identity.json.next").read_text())
        assert nxt["escrowKey"] != made["escrowKey"] and nxt["key"] == made["key"]
        assert json.loads(other.read_text()) == made, "the file in use stays"
        try:
            escrow_key(path)
        except SystemExit:
            pass
        else:
            raise AssertionError("an escrow key was made for a relay without escrow")
    print("identity.py self-test: ok")


def main(argv: list[str]) -> int:
    match argv[1:]:
        case ["--self-test"]:
            self_test()
        case ["create", *where, "--escrow"] | ["create", *where] if len(where) in (0, 2):
            name, host = where or [NAME, HOST]
            made = create(path_of(name), host, escrow=argv[-1] == "--escrow")
            print(f"identity key fingerprint {made}")
        case ["merge", *name] if len(name) <= 1:
            print(f"identity key fingerprint {merge(path_of(*name or [NAME]), sys.stdin.read())} (unchanged)")
        case ["escrow-key", *name] if len(name) <= 1:
            print(f"new escrow key fingerprint {escrow_key(path_of(*name or [NAME]))} in identity.json.next")
        case _:
            print(__doc__, file=sys.stderr)
            return 2
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
