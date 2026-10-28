"""Verify publisher ordering and failure behavior without remote writes."""
import importlib.util
import json
import os
from pathlib import Path
import runpy
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('publisher', ROOT / 'scripts/ci/publish-pacman-github.py')
PUBLISHER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PUBLISHER)


class GithubPublisherTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory(prefix='uc-publisher-test-')
        cls.base = Path(cls.tmp.name)
        cls.home = cls.base / 'gpg'
        cls.home.mkdir(mode=0o700)
        cls.environment = patch.dict(os.environ, GNUPGHOME=str(cls.home))
        cls.environment.start()
        subprocess.run(['gpg', '--batch', '--passphrase', '', '--quick-generate-key',
                        'Publisher test <test@example.invalid>', 'ed25519', 'sign', '0'],
                       check=True, capture_output=True)
        keys = subprocess.check_output(['gpg', '--with-colons', '--list-keys'], text=True)
        cls.fingerprint = next(line.split(':')[9] for line in keys.splitlines() if line.startswith('fpr:'))
        fixture = runpy.run_path(str(ROOT / 'scripts/tests/pacman_repo_acceptance.py'))['fixture']
        package = fixture(cls.base, '1.0.0-1')
        cls.snapshot = cls.base / 'snapshot'
        PUBLISHER.ASSEMBLE.assemble(package, cls.snapshot, 'x86_64', cls.fingerprint)

    @classmethod
    def tearDownClass(cls):
        subprocess.run(['gpgconf', '--kill', 'all'], check=True)
        cls.environment.stop()
        cls.tmp.cleanup()

    def execute(self, apply, release=None):
        calls = []
        actual = PUBLISHER.run
        def run(*args):
            if args[0] != 'gh':
                return actual(*args)
            calls.append(args)
            if args[1] == 'api':
                if '/releases?' in args[-1]:
                    return json.dumps([[release] if release else []])
                return json.dumps([{'assets': [
                    {'name': path.name, 'digest': PUBLISHER.digest(path)}
                    for path in (self.snapshot / 'x86_64').glob('*.pkg.tar.zst*')]}])
            if args[1:3] == ('release', 'download'):
                name = args[args.index('--pattern') + 1]
                output = Path(args[args.index('--dir') + 1])
                shutil.copyfile(self.snapshot / 'x86_64' / name, output / name)
            return ''
        with patch.object(PUBLISHER, 'run', run):
            PUBLISHER.publish(self.snapshot, 'x86_64', self.fingerprint, 'owner/repo', apply)
        return calls

    def test_dry_run_never_writes(self):
        self.assertTrue(all(call[1] == 'api' for call in self.execute(False)))

    def test_publish_packages_before_database_then_expose_draft(self):
        calls = self.execute(True)
        create = next(call for call in calls if call[1:3] == ('release', 'create'))
        self.assertIn('--draft', create)
        self.assertIn('--latest=false', create)
        uploads = [call for call in calls if call[1:3] == ('release', 'upload')]
        self.assertTrue(any(str(arg).endswith('.pkg.tar.zst') for arg in uploads[0]))
        self.assertIn('--clobber', uploads[-1])
        self.assertTrue(any(str(arg).endswith('uniclipboard.db') for arg in uploads[-1]))
        self.assertEqual(calls[-1][1:3], ('release', 'edit'))
        self.assertIn('--draft=false', calls[-1])

    def test_existing_equal_version_is_rejected(self):
        assets = [{'name': name} for name in ('uniclipboard.db', 'uniclipboard.db.sig')]
        with self.assertRaisesRegex(ValueError, 'strictly newer'):
            self.execute(True, {'tag_name': 'pacman-stable-x86_64', 'draft': False, 'assets': assets})

    def test_partial_or_immutable_channel_is_rejected(self):
        with self.assertRaisesRegex(ValueError, 'incomplete'):
            self.execute(True, {'tag_name': 'pacman-stable-x86_64', 'draft': False,
                                'assets': [{'name': 'uniclipboard.db'}]})
        with self.assertRaisesRegex(ValueError, 'mutable'):
            self.execute(True, {'tag_name': 'pacman-stable-x86_64', 'draft': False,
                                'immutable': True, 'assets': []})


if __name__ == '__main__':
    unittest.main()
