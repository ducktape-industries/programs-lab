#!/usr/bin/env python3
"""Run tools/publish-dl.py in a scratch repository whose history has a merge, over stand-in
bodies (a program with its view and notifier packed in, and a view alone), against a stand-in
for `aws` that keeps the bucket in a directory and logs each upload: the version is the
first-parent count, the manifest has its shape (the programs rev Cargo.toml pins, the wire id
read out of the views packed into a program), the upload order
ends with the version's manifest and then latest.json, a rerun skips what is there, a stage
whose file differs from its manifest or a version's file with other bytes stops the upload
before any upload, and latest.json never moves back.
   publish-dl-check.py   (run by `make dl-publish-check`)"""

import hashlib
import json
import os
import pathlib
import subprocess
import sys
import tempfile

PUBLISH = pathlib.Path(__file__).resolve().parent / "publish-dl.py"
WIRE = "0123456789abcdef"
REV = "a358ea23027f529298126f4543b23ee2b87af655"
SDK = "https://github.com/ducktape-industries/programs"

# `aws s3api head-object` and `aws s3 cp` over $BUCKET_DIR/<bucket>/<key>, the metadata and
# Cache-Control of each object beside it as <key>.head, each upload's key appended to $BUCKET_DIR/log
AWS = r"""#!/usr/bin/env python3
import json, os, pathlib, sys
root = pathlib.Path(os.environ["BUCKET_DIR"])
args = sys.argv[1:]
option = lambda name: args[args.index(name) + 1]
if args[:2] == ["s3api", "head-object"]:
    head = root / option("--bucket") / (option("--key") + ".head")
    if not head.exists():
        sys.exit("An error occurred (404) when calling the HeadObject operation: Not Found")
    print(head.read_text())
elif args[:2] == ["s3", "cp"] and args[3] == "-":
    sys.stdout.buffer.write((root / args[2].removeprefix("s3://")).read_bytes())
elif args[:2] == ["s3", "cp"]:
    key = args[3].removeprefix("s3://")
    target = root / key
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_bytes(pathlib.Path(args[2]).read_bytes())
    sha256 = option("--metadata").removeprefix("sha256=")
    head = {"CacheControl": option("--cache-control"), "Metadata": {"sha256": sha256}}
    (root / (key + ".head")).write_text(json.dumps(head))
    with open(root / "log", "a") as log:
        log.write(key.split("/", 1)[1] + "\n")
else:
    sys.exit(f"unexpected: aws {args}")
"""


def leb128(value):
    out = bytearray()
    while True:
        byte, value = value & 0x7F, value >> 7
        out.append(byte | (0x80 if value else 0))
        if not value:
            return bytes(out)


def custom(name, data):
    """A custom section: id 0, its size, its name and its bytes."""
    section = leb128(len(name)) + name.encode() + data
    return b"\0" + leb128(len(section)) + section


def view(name, wire=WIRE):
    """A wasm module whose only section is a view manifest (`view_wire::manifest`)."""
    text = f"ducktape.view.manifest\n{name}\nA view\nop,\n320\n{wire}\ntrade,\n\nnone".encode()
    return b"\0asm\x01\0\0\0" + custom("ducktape.view.manifest", text)


def packed(name):
    """A program with its view, describe module and notifier packed in, as `pack` packs one."""
    return (
        b"\0asm\x01\0\0\0"
        + custom("ducktape.view", view(name))
        + custom("ducktape.describe", b"\0asm\x01\0\0\0")
        + custom("ducktape.notify", view(f"{name} notices"))
    )


def cargo_toml(*revs):
    """A workspace whose dependencies pin programs at revs (its sdk-line)."""
    lines = ["[workspace.dependencies]"]
    lines += [f'dep{i} = {{ git = "{SDK}", rev = "{rev}" }}' for i, rev in enumerate(revs)]
    lines.append('borsh = { version = "1" }')
    return "\n".join(lines) + "\n"


def git(repo, *args):
    env = {**os.environ, "GIT_AUTHOR_DATE": "@1760000000 +0900", "GIT_COMMITTER_DATE": "@1760000000 +0900"}
    return subprocess.run(
        ["git", "-C", str(repo), *args], check=True, capture_output=True, text=True, env=env
    ).stdout.strip()


def check(condition, what):
    if not condition:
        sys.exit(f"publish-dl-check: FAIL: {what}")


def main():
    with tempfile.TemporaryDirectory() as scratch:
        scratch = pathlib.Path(scratch)
        repo, build, bucket, bin_dir = (scratch / name for name in ("repo", "build", "bucket", "bin"))
        for directory in (repo, build, bucket, bin_dir):
            directory.mkdir()
        (bin_dir / "aws").write_text(AWS)
        (bin_dir / "aws").chmod(0o755)

        # dev: a (the Cargo.toml), b, then a merge of side (c) — three first-parent commits, four in all
        git(repo, "init", "-q", "-b", "dev")
        git(repo, "config", "user.email", "check@ducktape.invalid")
        git(repo, "config", "user.name", "check")
        (repo / "Cargo.toml").write_text(cargo_toml(REV, REV))
        git(repo, "add", "Cargo.toml")
        git(repo, "commit", "-q", "-m", "a")
        git(repo, "commit", "-q", "--allow-empty", "-m", "b")
        git(repo, "checkout", "-q", "-b", "side")
        git(repo, "commit", "-q", "--allow-empty", "-m", "c")
        git(repo, "checkout", "-q", "dev")
        git(repo, "merge", "-q", "--no-ff", "-m", "merge side", "side")
        commit = git(repo, "rev-parse", "HEAD")
        check(git(repo, "rev-list", "--count", "HEAD") == "4", "the scratch history has four commits")

        # the program is the only body that speaks the wire: its wire id is read out of the
        # view packed into it
        files = {"trade.wasm": packed("Trade"), "notes.txt": b"not wasm"}
        for name, data in files.items():
            (build / name).write_bytes(data)
        env = {
            **os.environ,
            "PATH": f"{bin_dir}{os.pathsep}{os.environ['PATH']}",
            "BUCKET_DIR": str(bucket),
            "R2_ACCOUNT_ID": "account",
            "R2_BUCKET": "releases",
        }

        stage_dir = scratch / "stage"

        def publish(*args):
            command = [sys.executable, str(PUBLISH), *args]
            if args[0] == "stage":
                command += [str(stage_dir), *(str(build / name) for name in files)]
            return subprocess.run(command, cwd=repo, env=env, capture_output=True, text=True)

        def log():
            return (bucket / "log").read_text().splitlines()

        order = [f"programs-lab/dev/3/{name}" for name in sorted(files)]
        order += ["programs-lab/dev/3/manifest.json", "programs-lab/dev/latest.json"]

        staged = publish("stage", "dev", "fast")
        check(staged.returncode == 0, f"the stage is written: {staged.stderr}")
        printed = [line.split("  ")[0] for line in staged.stdout.splitlines()[1:]]
        check(printed == order, f"the stage prints every file, then the manifest, then latest.json: {printed}")
        check(sorted(path.name for path in stage_dir.iterdir()) == sorted([*files, "manifest.json"]), "the stage")
        manifest = json.loads((stage_dir / "manifest.json").read_text())
        expected = {
            "repo": "programs-lab",
            "channel": "dev",
            "version": 3,
            "display": "dev-3",
            "commit": commit,
            "profile": "fast",
            "built_at": "2025-10-09T08:53:20Z",
            "wire_id": WIRE,
            "programs_rev": REV,
            "artifacts": {
                name: {"sha256": hashlib.sha256(data).hexdigest(), "bytes": len(data)}
                for name, data in sorted(files.items())
            },
        }
        check(manifest == expected, f"the manifest: {json.dumps(manifest, indent=2)}")
        check(list(manifest["artifacts"]) == sorted(files), "the artifacts are sorted by name")
        check(publish("stage", "main", "fast").returncode != 0, "main refuses the fast profile's bytes")
        check(publish("stage", "dev", "release").returncode != 0, "dev refuses the release profile's bytes")
        (build / "notes.txt").write_bytes(view("Notes", "fedcba9876543210"))
        run = publish("stage", "dev", "fast")
        check(run.returncode != 0 and "one wire" in run.stderr, f"views on two wires are refused: {run.stderr}")
        (build / "notes.txt").write_bytes(files["notes.txt"])
        (repo / "Cargo.toml").write_text(cargo_toml(REV, "0" * 40))
        git(repo, "commit", "-q", "-am", "two revs")
        run = publish("stage", "dev", "fast")
        check(run.returncode != 0 and "one programs rev" in run.stderr, f"two programs revs are refused: {run.stderr}")
        git(repo, "reset", "-q", "--hard", commit)
        check(publish("stage", "dev", "fast").returncode == 0, "the stage is written again")

        run = publish("upload", str(stage_dir))
        check(run.returncode == 0, f"the upload succeeds: {run.stderr}")
        check(log() == order, f"every file, then the manifest, then latest.json: {log()}")
        latest = bucket / "releases/programs-lab/dev/latest.json"
        check(json.loads(latest.read_text()) == manifest, "latest.json is the version's manifest")
        heads = {key: json.loads((bucket / f"releases/{key}.head").read_text())["CacheControl"] for key in order}
        check(all(heads[key] == "public, max-age=31536000, immutable" for key in order[:-1]), f"immutable: {heads}")
        check(heads["programs-lab/dev/latest.json"] == "public, max-age=60", f"latest.json lives a minute: {heads}")

        run = publish("upload", str(stage_dir))
        check(run.returncode == 0, f"a rerun succeeds: {run.stderr}")
        check(log() == order + ["programs-lab/dev/latest.json"], f"a rerun skips the version's files: {log()}")
        uploaded = log()

        (stage_dir / "trade.wasm").write_bytes(packed("Trade, damaged"))
        run = publish("upload", str(stage_dir))
        check(run.returncode != 0 and "trade.wasm" in run.stderr, f"a damaged stage is refused: {run.stderr}")
        check(log() == uploaded, "a damaged stage uploads nothing")

        (build / "trade.wasm").write_bytes(packed("Trade, rebuilt"))
        check(publish("stage", "dev", "fast").returncode == 0, "the rebuilt file is staged")
        run = publish("upload", str(stage_dir))
        check(
            run.returncode != 0 and "programs-lab/dev/3/trade.wasm" in run.stderr,
            f"other bytes are refused: {run.stderr}",
        )
        check(log() == uploaded, "a refused upload uploads nothing")
        (build / "trade.wasm").write_bytes(files["trade.wasm"])
        check(publish("stage", "dev", "fast").returncode == 0, "the stage is written again")

        latest.write_text(json.dumps({**manifest, "version": 9}))
        run = publish("upload", str(stage_dir))
        check(run.returncode == 0, f"an older version publishes: {run.stderr}")
        check(json.loads(latest.read_text())["version"] == 9, "latest.json never moves back")
        check(log() == uploaded, "nothing of an older version is uploaded again")
    print("publish-dl: version, manifest, upload order, rerun, refusals and latest.json hold")


if __name__ == "__main__":
    main()
