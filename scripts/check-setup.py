#!/usr/bin/env python3
"""Run the real bootstrap against local archive fixtures, without sudo/network."""
import hashlib
import io
import os
from pathlib import Path
import re
import shlex
import subprocess
import tarfile
import tempfile
import unittest

REPOSITORY = Path(__file__).resolve().parent.parent
SOURCE = (REPOSITORY / 'setup.sh').read_text()
PREFIX = SOURCE.rsplit('\nsetup::installation::run "$@"', 1)[0]


@unittest.skipUnless(os.uname().sysname == 'Darwin', 'macOS bootstrap')
class Setup(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='galaxybridge-setup-test-')
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.marker = self.root / 'installed'
        self.download = self.root / 'download-destination'
        self.tools = self.root / 'tools-called'
        self.archive = self.root / 'fixture.tar.gz'

    def fixture(self, code=0):
        installer = ('#!/bin/bash\nprintf "%s" "$*" > ' + shlex.quote(str(self.marker)) + f'\nexit {code}\n').encode()
        with tarfile.open(self.archive, 'w:gz') as archive:
            item = tarfile.TarInfo('galaxybridge-0.2.0-macos-arm64/install.sh')
            item.size = len(installer)
            item.mode = 0o755
            archive.addfile(item, io.BytesIO(installer))
        return hashlib.sha256(self.archive.read_bytes()).hexdigest()

    def run_setup(self, args=(), wrong_hash=False, download_error=False, tool_error=False, install_code=0):
        digest = self.fixture(install_code)
        if wrong_hash:
            digest = '0' * 64
        prefix = re.sub(r'readonly setup_sha256=[a-f0-9]{64}', 'readonly setup_sha256=' + digest, PREFIX)
        overrides = '\nsetup::platform::check() { :; }\n'
        overrides += 'setup::download::get() {\n'
        overrides += 'printf "%s" "$2" > ' + shlex.quote(str(self.download)) + '\n'
        overrides += '/bin/cp ' + shlex.quote(str(self.archive)) + ' "$2"\n'
        overrides += f'return {22 if download_error else 0}\n}}\n'
        overrides += 'setup::toolchain::check() { /usr/bin/touch ' + shlex.quote(str(self.tools))
        overrides += f'; return {1 if tool_error else 0}; }}\n'
        result = subprocess.run(['/bin/bash', '-c', prefix + overrides + '\nsetup::installation::run "$@"', 'setup-test', *args], capture_output=True, text=True, timeout=10)
        if self.download.exists():
            self.assertFalse(Path(self.download.read_text()).parent.exists(), 'private download directory leaked')
        return result

    def test_verified_archive_installs_automatic_mode_and_cleans_private_files(self):
        result = self.run_setup()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.marker.read_text(), '--auto')
        self.assertTrue(self.tools.exists())
        self.assertIn('Setup complete.', result.stdout)

    def test_manual_mode_does_not_enable_background_service(self):
        result = self.run_setup(['--manual'])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.marker.read_text(), '')

    def test_corrupt_archive_never_reaches_installer_or_tool_installation(self):
        result = self.run_setup(wrong_hash=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('checksum mismatch', result.stderr)
        self.assertFalse(self.marker.exists())
        self.assertFalse(self.tools.exists())

    def test_partial_failed_download_never_runs_even_when_bytes_match(self):
        result = self.run_setup(download_error=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.marker.exists())
        self.assertFalse(self.tools.exists())

    def test_missing_apple_tools_prevent_privileged_handoff(self):
        result = self.run_setup(tool_error=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.marker.exists())

    def test_failed_installation_is_not_reported_as_success(self):
        result = self.run_setup(install_code=17)
        self.assertEqual(result.returncode, 17)
        self.assertNotIn('Setup complete.', result.stdout)

    def test_unknown_option_and_help_do_not_download(self):
        for args in (['--unknown'], ['--manual=1'], ['--version', '0.1.0']):
            result = self.run_setup(args)
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse(self.download.exists())
        result = self.run_setup(['--help'])
        self.assertEqual(result.returncode, 0)
        self.assertIn('Usage:', result.stdout)
        self.assertFalse(self.download.exists())

    def test_readme_download_failure_does_not_execute_partial_bootstrap(self):
        command = next(line for line in (REPOSITORY / 'README.md').read_text().splitlines() if line.startswith('/bin/bash -c '))
        fake_curl = self.root / 'curl'
        # A transport failure can still output a syntactically complete script.
        fake_curl.write_text('#!/bin/bash\nprintf "%s\\n" ' + shlex.quote('/usr/bin/touch ' + shlex.quote(str(self.marker))) + '\nexit 22\n')
        fake_curl.chmod(0o755)
        command = command.replace('/usr/bin/curl', str(fake_curl))
        result = subprocess.run(['/bin/bash', '-c', command], capture_output=True, text=True, timeout=5)
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.marker.exists())

    def test_download_disables_curl_config_and_has_kernel_file_limit(self):
        # Exercise the actual download subshell, replacing only curl with a
        # local program that reports argv and inherited kernel resource limits.
        probe = self.root / 'probe'
        probe.write_text('#!/usr/bin/python3\nimport json,resource,sys\nprint(json.dumps([sys.argv[1:],resource.getrlimit(resource.RLIMIT_FSIZE),resource.getrlimit(resource.RLIMIT_CORE)]))\n')
        probe.chmod(0o755)
        prefix = PREFIX.replace('/usr/bin/curl', shlex.quote(str(probe)))
        result = subprocess.run(['/bin/bash', '-c', prefix + '\nsetup::download::get https://example.invalid/archive /unused'], capture_output=True, text=True, timeout=5)
        self.assertEqual(result.returncode, 0, result.stderr)
        import json
        args, limits, core = json.loads(result.stdout)
        self.assertEqual(args[0], '--disable')
        self.assertEqual(args[args.index('--proto-redir') + 1], '=https')
        self.assertTrue(0 < limits[0] <= 16 * 1024 * 1024)
        self.assertTrue(0 < limits[1] <= 16 * 1024 * 1024)
        self.assertEqual(core, [0, 0])


if __name__ == '__main__':
    unittest.main()
