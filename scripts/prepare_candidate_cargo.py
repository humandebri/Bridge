#!/usr/bin/env python3
"""Prepare a trusted workspace with only the reviewed Cargo lockfile overlaid."""

from __future__ import annotations

import argparse
from pathlib import Path
import re
import subprocess
import tarfile
import tomllib


CRATES_IO = "registry+https://github.com/rust-lang/crates.io-index"


def prepare(trusted: Path, candidate_lock: Path, destination: Path) -> None:
    if candidate_lock.is_symlink() or not candidate_lock.is_file():
        raise ValueError("candidate Cargo.lock must be a regular file")
    data = candidate_lock.read_bytes()
    lock = tomllib.loads(data.decode("utf-8"))
    packages = lock.get("package")
    if not isinstance(packages, list) or not packages:
        raise ValueError("candidate Cargo.lock has no packages")
    for package in packages:
        source = package.get("source")
        if source is not None and source != CRATES_IO:
            raise ValueError("candidate Cargo.lock contains an unsupported source")
        if source == CRATES_IO and not re.fullmatch(r"[0-9a-f]{64}", package.get("checksum", "")):
            raise ValueError("candidate registry package has an invalid checksum")
    if destination.exists() or destination.is_symlink():
        raise ValueError("trusted Cargo workspace destination already exists")
    destination.mkdir()
    with subprocess.Popen(
        ["git", "-C", str(trusted), "archive", "--format=tar", "HEAD"],
        stdout=subprocess.PIPE,
    ) as archive:
        assert archive.stdout is not None
        with tarfile.open(fileobj=archive.stdout, mode="r|") as stream:
            stream.extractall(destination, filter="data")
        if archive.wait() != 0:
            raise ValueError("could not archive trusted Cargo workspace")
    lock_path = destination / "Cargo.lock"
    if lock_path.is_symlink() or not lock_path.is_file():
        raise ValueError("trusted Cargo.lock must be a regular file")
    lock_path.write_bytes(data)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--trusted", type=Path, required=True)
    parser.add_argument("--candidate-lock", type=Path, required=True)
    parser.add_argument("--destination", type=Path, required=True)
    args = parser.parse_args()
    prepare(args.trusted, args.candidate_lock, args.destination)


if __name__ == "__main__":
    main()
