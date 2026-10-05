#!/usr/bin/env python3
"""Prepare immutable candidate/official-previous inputs; never install on the host."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import tomllib


def command(*args):
    return subprocess.check_output(args, text=True).strip()


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def version(tag):
    if not re.fullmatch(r"v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)", tag):
        raise ValueError("expected stable vX.Y.Z tag")
    return tuple(map(int, tag[1:].split(".")))


def previous_release(releases, candidate):
    eligible = [r["tagName"] for r in releases
                if not r["isDraft"] and not r["isPrerelease"]
                and re.fullmatch(r"v\d+\.\d+\.\d+", r["tagName"])
                and version(r["tagName"]) < version(candidate)]
    if not eligible:
        raise ValueError("no previous official stable release")
    return max(eligible, key=version)


def verify_release(directory, tag, target):
    manifest = json.loads((directory / "release.json").read_text())
    assert manifest["schemaVersion"] == 1 and manifest["tag"] == tag
    assert manifest["version"] == tag[1:]
    artifact, = [a for a in manifest["artifacts"] if a["target"] == target]
    filename = f"ocd-{tag}-{target}"
    assert artifact["filename"] == filename
    binary = directory / filename
    assert binary.stat().st_size == artifact["bytes"]
    assert digest(binary) == artifact["sha256"]
    sums = dict((name, checksum) for checksum, name in
                (line.split() for line in (directory / "SHA256SUMS").read_text().splitlines()))
    assert sums[filename] == artifact["sha256"]
    assert sums["release.json"] == digest(directory / "release.json")


def verify_migrations(tag):
    paths = command("git", "ls-tree", "-r", "--name-only", tag, "--",
                    "crates/storage/refinery-migrations").splitlines()
    assert paths, "official tag contains no migration evidence"
    for name in paths:
        if name.endswith(".sql"):
            published = subprocess.check_output(["git", "show", f"{tag}:{name}"])
            assert Path(name).read_bytes() == published, f"published migration changed: {name}"


def prepare(args):
    report = json.loads(args.report.read_text())
    tag = "v" + report["version"]
    assert report["schemaVersion"] == 1 and report["target"] == args.target
    workspace = tomllib.loads(Path("Cargo.toml").read_text())["workspace"]["package"]
    assert report["version"] == workspace["version"]
    assert report["revision"] == command("git", "rev-parse", "HEAD")
    assert digest(args.candidate) == report["sha256"]
    assert args.candidate.stat().st_size == report["bytes"]
    lock = Path("packages/runtime/workerd.lock.json")
    assert report["workerdLockSha256"] == digest(lock)
    assert report["workerd"] == json.loads(lock.read_text())["release"]
    releases = json.loads(command("gh", "release", "list", "--repo", "elliothux/open-compute",
                                 "--limit", "100", "--json", "tagName,isDraft,isPrerelease"))
    previous = previous_release(releases, tag)
    verify_migrations(previous)
    previous_dir = args.output / "download" / previous
    previous_dir.mkdir(parents=True, exist_ok=False)
    command("gh", "release", "download", previous, "--repo", "elliothux/open-compute",
            "--dir", str(previous_dir), "--pattern", "release.json", "--pattern", "SHA256SUMS",
            "--pattern", f"ocd-{previous}-{args.target}")
    verify_release(previous_dir, previous, args.target)
    candidate_dir = args.output / "download" / tag
    candidate_dir.mkdir(parents=True, exist_ok=False)
    filename = f"ocd-{tag}-{args.target}"
    shutil.copyfile(args.candidate, candidate_dir / filename)
    os_name, arch = args.target.split("-")
    manifest = {"schemaVersion": 1, "tag": tag, "version": report["version"],
                "gitRevision": report["revision"], "workerdRelease": report["workerd"],
                "workerdLockSha256": report["workerdLockSha256"], "artifacts": [{
                    "target": args.target, "os": os_name, "arch": arch,
                    "filename": filename, "bytes": report["bytes"], "sha256": report["sha256"]}]}
    (candidate_dir / "release.json").write_text(json.dumps(manifest))
    (candidate_dir / "SHA256SUMS").write_text(
        f'{report["sha256"]}  {filename}\n{digest(candidate_dir / "release.json")}  release.json\n')
    verify_release(candidate_dir, tag, args.target)
    (args.output / "inputs.json").write_text(json.dumps({"candidate": tag, "previous": previous,
                                                        "target": args.target}, indent=2))
    print(f"verified {previous} -> {tag}, {args.target}, published migrations unchanged")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--target", choices=["darwin-arm64", "linux-arm64", "linux-x64"], required=True)
    parser.add_argument("--output", type=Path, required=True)
    prepare(parser.parse_args())
