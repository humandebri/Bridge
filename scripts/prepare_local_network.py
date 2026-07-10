#!/usr/bin/env python3
"""Repository local-network setup: select a free ICP gateway port without dependencies."""

from __future__ import annotations

import argparse
import socket
from pathlib import Path

DEFAULT_PORT = 8000
MAX_PORT = 8100


def port_is_free(port: int) -> bool:
    """Return whether both IPv4 loopback and wildcard binding can use the candidate port."""

    for host in ("127.0.0.1", "0.0.0.0"):
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as candidate:
            candidate.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            try:
                candidate.bind((host, port))
            except OSError:
                return False
    return True


def choose_port() -> int:
    """Prefer the ICP CLI default and otherwise return the first nearby free port."""

    for port in range(DEFAULT_PORT, MAX_PORT + 1):
        if port_is_free(port):
            return port
    raise RuntimeError(f"no free gateway port in {DEFAULT_PORT}..{MAX_PORT}")


def render_config(source: str, port: int) -> str:
    """Replace or append networks[].gateway.port for the managed local network."""

    lines = source.splitlines()
    networks_index: int | None = None
    networks_end: int | None = None
    local_index: int | None = None
    local_end: int | None = None
    gateway_index: int | None = None
    port_index: int | None = None

    for index, line in enumerate(lines):
        if line == "networks:":
            networks_index = index
            break

    if networks_index is None:
        suffix = [] if not lines or lines[-1] == "" else [""]
        lines.extend(
            [
                *suffix,
                "networks:",
                "  - name: local",
                "    mode: managed",
                "    gateway:",
                f"      port: {port}",
            ]
        )
        return "\n".join(lines) + "\n"

    networks_end = len(lines)
    for index in range(networks_index + 1, len(lines)):
        line = lines[index]
        if line and not line[0].isspace():
            networks_end = index
            break
        if line == "  - name: local":
            local_index = index

    if local_index is None:
        lines[networks_end:networks_end] = [
            "  - name: local",
            "    mode: managed",
            "    gateway:",
            f"      port: {port}",
        ]
        return "\n".join(lines) + "\n"

    local_end = networks_end
    for index in range(local_index + 1, networks_end):
        if lines[index].startswith("  - "):
            local_end = index
            break
        if lines[index].startswith("    gateway:"):
            if lines[index] != "    gateway:":
                raise ValueError("inline gateway mappings are unsupported")
            gateway_index = index

    if gateway_index is None:
        lines[local_end:local_end] = ["    gateway:", f"      port: {port}"]
        return "\n".join(lines) + "\n"

    for index in range(gateway_index + 1, local_end):
        line = lines[index]
        if line and len(line) - len(line.lstrip()) <= 4:
            break
        if line.startswith("      port:"):
            port_index = index

    if port_index is None:
        lines.insert(gateway_index + 1, f"      port: {port}")
    else:
        lines[port_index] = f"      port: {port}"

    return "\n".join(lines) + "\n"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--project-root", type=Path, required=True)
    parser.add_argument("--write", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    config_path = args.project_root.resolve() / "icp.yaml"
    source = config_path.read_text(encoding="utf-8")
    port = choose_port()
    rendered = render_config(source, port)

    if args.write and rendered != source:
        config_path.write_text(rendered, encoding="utf-8")

    print(port)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
