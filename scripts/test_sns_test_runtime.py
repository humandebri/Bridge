#!/usr/bin/env python3
"""Exercise the installer download boundary without network access or GitHub auth."""
import io
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
import urllib.error

ROOT = Path(__file__).resolve().parents[1]
INSTALLER = ROOT / 'scripts/prepare-sns-test-runtime.sh'
CODE = compile(INSTALLER.read_text().split("<<'PY'\n", 1)[1].rsplit('\nPY', 1)[0], str(INSTALLER), 'exec')
URL = 'https://github.com/dfinity/ic/releases/download/release-2026-09-10_03-28--all-in-one-node/canisters.tar'


class RuntimeDownloadTests(unittest.TestCase):
    def test_failed_download_never_publishes_or_extracts(self):
        for failure in (urllib.error.URLError('offline'), OSError('interrupted')):
            with self.subTest(failure=failure), tempfile.TemporaryDirectory() as directory:
                with patch.object(sys, 'argv', ['installer', directory]), \
                     patch('urllib.request.urlopen', side_effect=failure) as request, \
                     patch('subprocess.check_output') as didc:
                    with self.assertRaises(type(failure)):
                        exec(CODE, {})
                    request.assert_called_once_with(URL, timeout=120)
                    didc.assert_not_called()
                    self.assertEqual(list(Path(directory).iterdir()), [])

    def test_hash_mismatch_never_publishes_or_extracts(self):
        with tempfile.TemporaryDirectory() as directory:
            with patch.object(sys, 'argv', ['installer', directory]), \
                 patch('urllib.request.urlopen', return_value=io.BytesIO(b'bad archive')) as request, \
                 patch('subprocess.check_output') as didc:
                with self.assertRaisesRegex(SystemExit, 'differs from the pinned'):
                    exec(CODE, {})
                request.assert_called_once_with(URL, timeout=120)
                didc.assert_not_called()
                self.assertEqual(list(Path(directory).iterdir()), [])

    def test_corrupt_cached_archive_is_rejected_without_download(self):
        with tempfile.TemporaryDirectory() as directory:
            (Path(directory) / 'canisters.tar').write_bytes(b'corrupt cache')
            with patch.object(sys, 'argv', ['installer', directory]), \
                 patch('urllib.request.urlopen') as request, \
                 patch('subprocess.check_output') as didc:
                with self.assertRaisesRegex(SystemExit, 'differs from the pinned'):
                    exec(CODE, {})
                request.assert_not_called()
                didc.assert_not_called()
                self.assertEqual(len(list(Path(directory).iterdir())), 1)


if __name__ == '__main__':
    unittest.main()
