"""Exercise the real PKGBUILD version function with tagged and untagged Git histories."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class AurPackageTests(unittest.TestCase):
    def test_renderer_uses_pkgbuild_version_and_handles_missing_tags(self):
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory) / 'repo'
            package = repo / 'packaging/aur/uniclipboard-git'
            package.mkdir(parents=True)
            shutil.copyfile(ROOT / 'packaging/aur/uniclipboard-git/PKGBUILD', package / 'PKGBUILD')
            def git(*args):
                return subprocess.check_output(['git', '-C', str(repo), *args], text=True).strip()
            git('init', '-q')
            git('config', 'user.name', 'Test')
            git('config', 'user.email', 'test@example.invalid')
            git('add', '.')
            git('commit', '-qm', 'Initial packaging')
            commit = git('rev-parse', '--short=7', 'HEAD')
            output = Path(directory) / 'PKGBUILD'
            def render():
                return subprocess.check_output(['bash', str(ROOT / 'scripts/ci/render-aur-package.sh'),
                                                str(repo), str(output)], text=True).strip()
            self.assertEqual(render(), f'0.0.0.r1.g{commit}')
            git('tag', 'v1.4.0-alpha.1')
            self.assertEqual(render(), f'1.4.0.alpha.1.r0.g{commit}')
            git('commit', '--allow-empty', '-qm', 'Next source commit')
            version = render()
            self.assertRegex(version, r'^1\.4\.0\.alpha\.1\.r1\.g[0-9a-f]{7}$')
            self.assertIn(f'pkgver={version}\n', output.read_text())
            self.assertIn('https://github.com/UniClipboard/UniClipboard.git', output.read_text())


class StablePackageTests(unittest.TestCase):
    def test_stable_renderer_requires_exact_version_tag(self):
        import json
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory) / 'repo'
            recipe = repo / 'packaging/aur/uniclipboard-git'
            recipe.mkdir(parents=True)
            shutil.copyfile(ROOT / 'packaging/aur/uniclipboard-git/PKGBUILD', recipe / 'PKGBUILD')
            app = repo / 'apps/gui-go/app.json'
            app.parent.mkdir(parents=True)
            app.write_text(json.dumps({'version': '1.5.0'}))
            def git(*args):
                return subprocess.check_output(['git', '-C', str(repo), *args], text=True).strip()
            git('init', '-q')
            git('config', 'user.name', 'Test')
            git('config', 'user.email', 'test@example.invalid')
            git('add', '.')
            git('commit', '-qm', 'Stable source')
            output = Path(directory) / 'PKGBUILD'
            def render():
                return subprocess.run(['bash', str(ROOT / 'scripts/ci/render-arch-release.sh'),
                                       str(repo), str(output)], capture_output=True, text=True)
            self.assertNotEqual(render().returncode, 0)
            git('tag', 'v1.5.0')
            self.assertEqual(render().returncode, 0)
            values = subprocess.check_output(['bash', '-c',
                'source "$1"; declare -p pkgname pkgver conflicts source; '
                '! declare -F pkgver; declare -F build package', '_', str(output)], text=True)
            self.assertIn('pkgname="uniclipboard"', values)
            self.assertIn('pkgver="1.5.0"', values)
            self.assertIn('uniclipboard-git', values)
            self.assertIn('#commit=' + git('rev-parse', 'HEAD'), values)
            git('commit', '--allow-empty', '-qm', 'After release')
            self.assertNotEqual(render().returncode, 0)


if __name__ == '__main__':
    unittest.main()
