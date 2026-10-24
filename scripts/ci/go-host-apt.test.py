#!/usr/bin/env python3
"""Focused cache validity tests; real apt cold/hot paths are verified separately."""

import copy
import importlib.util
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('apt_cache', Path(__file__).with_name('go-host-apt.py'))
apt_cache = importlib.util.module_from_spec(spec)
spec.loader.exec_module(apt_cache)


class AptDownloadsTest(unittest.TestCase):
    def setUp(self):
        self.payload = b'authenticated archive bytes'
        self.downloads = {'test_1_amd64.deb': {
            'uri': 'https://example.invalid/test.deb', 'size': len(self.payload),
            'sha256': apt_cache.digest(self.payload)}}

    def test_print_uris_requires_sha256_and_safe_names(self):
        checksum = apt_cache.digest(self.payload)
        line = f"'https://example.invalid/test.deb' test_1_amd64.deb {len(self.payload)} SHA256:{checksum}"
        self.assertEqual(apt_cache.manifest('Reading package lists...\n' + line), self.downloads)
        for bad in [line.replace('SHA256:', 'MD5Sum:'), line.replace(' test_1', ' ../test_1')]:
            with self.assertRaises(ValueError):
                apt_cache.manifest(bad)
        self.assertEqual(apt_cache.manifest('0 upgraded, 0 newly installed.'), {})

    def test_cold_hot_corrupt_and_obsolete_downloads(self):
        with tempfile.TemporaryDirectory() as directory:
            archives = Path(directory)
            apt_cache.validate(archives, self.downloads)  # Cold cache is valid.
            archive = archives / 'test_1_amd64.deb'
            archive.write_bytes(self.payload)
            apt_cache.validate(archives, self.downloads)
            self.assertEqual(archive.read_bytes(), self.payload)
            archive.write_bytes(b'x' * len(self.payload))  # Same size, wrong hash.
            apt_cache.validate(archives, self.downloads)
            self.assertFalse(archive.exists())
            archive.write_bytes(b'short')
            apt_cache.validate(archives, self.downloads)
            self.assertFalse(archive.exists())
            archive.write_bytes(self.payload)
            apt_cache.validate(archives, {})
            self.assertFalse(archive.exists())

    def test_symlinks_fail_closed(self):
        with tempfile.TemporaryDirectory() as directory:
            archives = Path(directory)
            (archives / 'test_1_amd64.deb').symlink_to('/etc/passwd')
            with self.assertRaises(ValueError):
                apt_cache.validate(archives, self.downloads)

    def test_resolution_os_arch_packages_and_sources_invalidate_key(self):
        inputs = [self.downloads, 'Ubuntu 24.04', 'amd64', {'ubuntu.sources': 'noble'}, ['test']]
        key = apt_cache.cache_key(*inputs)
        self.assertEqual(key, apt_cache.cache_key(*copy.deepcopy(inputs)))
        for field, replacement in [(0, {}), (1, 'Ubuntu 26.04'), (2, 'arm64'),
                                   (3, {'ubuntu.sources': 'new mirror'}), (4, ['other'])]:
            changed = copy.deepcopy(inputs)
            changed[field] = replacement
            self.assertNotEqual(key, apt_cache.cache_key(*changed))
        for field, replacement in [('sha256', '0' * 64), ('uri', 'https://other.invalid/test.deb'), ('size', 99)]:
            changed = copy.deepcopy(inputs)
            changed[0]['test_1_amd64.deb'][field] = replacement
            self.assertNotEqual(key, apt_cache.cache_key(*changed))


if __name__ == '__main__':
    unittest.main()
