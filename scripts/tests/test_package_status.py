"""Contract tests for external registry responses and explicit failure states."""
import gzip
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import xml.etree.ElementTree as ET

spec = importlib.util.spec_from_file_location('package_status', Path(__file__).parents[1] / 'package-status.py')
status = importlib.util.module_from_spec(spec)
spec.loader.exec_module(status)


class PackageStatusTests(unittest.TestCase):
    def test_versions_are_numeric_and_stable_follows_prerelease(self):
        self.assertEqual(status.latest(['1.9.0', '1.10.0-alpha.2', '1.10.0-alpha.10', '1.10.0']), '1.10.0')
        self.assertEqual(status.latest(['1.10.0-alpha.2', '1.10.0-alpha.10']), '1.10.0-alpha.10')

    def test_network_failure_does_not_claim_unpublished_or_reuse_a_version(self):
        def fail():
            raise TimeoutError('registry unreachable')
        name, result = status.probe(('npm', fail))
        self.assertEqual(name, 'npm')
        self.assertEqual(result['status'], 'unavailable')
        self.assertIsNone(result['version'])

    def test_missing_channel_is_unpublished(self):
        self.assertEqual(status.probe(('npm-alpha', lambda: None))[1]['status'], 'not-published')

    def test_malformed_metadata_is_an_error(self):
        self.assertEqual(status.probe(('npm', lambda: ''))[1]['status'], 'unavailable')

    def test_snap_partial_rollout_names_the_architectures(self):
        entries = [{'version': version, 'channel': {'risk': 'edge', 'track': 'latest', 'architecture': arch}}
                   for arch, version in [('amd64', '1.4.0-alpha.1'), ('arm64', '1.3.1')]]
        with patch.object(status, 'document', return_value={'channel-map': entries}):
            self.assertEqual(status.snap_version('edge'), 'amd64: 1.4.0-alpha.1 / arm64: 1.3.1')
            with self.assertRaises(status.NotPublished):
                status.snap_version('stable')

    def test_chocolatey_ignores_unapproved_and_unlisted_packages(self):
        xml = '''<feed xmlns="http://www.w3.org/2005/Atom" xmlns:d="urn:properties">
        <entry><d:Version>1.3.0</d:Version><d:IsApproved>true</d:IsApproved></entry>
        <entry><d:Version>1.4.0</d:Version><d:IsApproved>false</d:IsApproved></entry>
        <entry><d:Version>1.5.0</d:Version><d:IsApproved>true</d:IsApproved><d:Listed>false</d:Listed></entry>
        </feed>'''
        with patch.object(status, 'fetch', return_value=xml.encode()):
            self.assertEqual(status.chocolatey_version(), '1.3.0')

    def test_copr_reads_compressed_repository_metadata(self):
        repomd = b'<repomd xmlns="http://linux.duke.edu/metadata/repo"><data type="primary"><location href="repodata/primary.xml.gz"/></data></repomd>'
        primary = b'<metadata xmlns="http://linux.duke.edu/metadata/common"><package><name>uniclipboard</name><version ver="1.3.1" rel="1.fc44"/></package><package><name>other</name><version ver="9.0.0" rel="1"/></package></metadata>'
        with patch.object(status, 'fetch', side_effect=[repomd, gzip.compress(primary)]):
            self.assertEqual(status.copr_version('uniclipboard'), '1.3.1-1.fc44')

    def test_flathub_ignores_upcoming_releases(self):
        releases = [{'version': '1.3.1', 'type': 'stable'},
                    {'version': '1.4.0-alpha.1', 'type': 'development'}]
        with patch.object(status, 'document', return_value={'releases': releases}):
            self.assertEqual(status.flathub_version(), '1.3.1')

    def test_snapshot_replaces_old_success_and_escapes_badge_content(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            with patch.object(status, 'CHANNELS', {'test': lambda: '1.0<&"'}):
                status.collect(output)
            svg = output / 'badges/test.svg'
            ET.fromstring(svg.read_text())
            self.assertIn('&lt;', svg.read_text())
            with patch.object(status, 'CHANNELS', {'test': lambda: None}):
                status.collect(output)
            self.assertNotIn('1.0', svg.read_text())
            self.assertIn('未发布', svg.read_text())


if __name__ == '__main__':
    unittest.main()
