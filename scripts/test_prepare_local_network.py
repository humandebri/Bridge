#!/usr/bin/env python3
"""Unit tests for deterministic, dependency-free ICP gateway configuration edits."""

import unittest

from prepare_local_network import render_config


class RenderConfigTests(unittest.TestCase):
    def test_appends_managed_local_network(self) -> None:
        source = "canisters: []\n"
        self.assertEqual(
            render_config(source, 8000),
            "canisters: []\n\nnetworks:\n  - name: local\n    mode: managed\n"
            "    gateway:\n      port: 8000\n",
        )

    def test_replaces_existing_port(self) -> None:
        source = (
            "networks:\n  - name: local\n    mode: managed\n    version: pinned\n    gateway:\n"
            "      port: 8001\ncanisters: []\n"
        )
        self.assertEqual(
            render_config(source, 8000),
            "networks:\n  - name: local\n    mode: managed\n    version: pinned\n    gateway:\n"
            "      port: 8000\ncanisters: []\n",
        )

    def test_preserves_other_gateway_fields(self) -> None:
        source = (
            "networks:\n  - name: local\n    mode: managed\n    gateway:\n"
            "      domains: [localhost]\ncanisters: []\n"
        )
        self.assertEqual(
            render_config(source, 8002),
            "networks:\n  - name: local\n    mode: managed\n    gateway:\n"
            "      port: 8002\n      domains: [localhost]\ncanisters: []\n",
        )

    def test_rejects_inline_gateway_mapping(self) -> None:
        with self.assertRaisesRegex(ValueError, "inline gateway"):
            render_config(
                "networks:\n  - name: local\n    gateway: { port: 8000 }\n",
                8001,
            )


if __name__ == "__main__":
    unittest.main()
