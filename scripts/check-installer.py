#!/usr/bin/env python3
"""Non-privileged installer input checks. Never runs installation or sudo."""
from pathlib import Path
import subprocess
import shlex
import tempfile
import unittest

REPOSITORY = Path(__file__).resolve().parent.parent


class InstallerArguments(unittest.TestCase):
    def run_script(self, script, arguments):
        return subprocess.run(
            ["/bin/bash", str(REPOSITORY / script), *arguments],
            text=True, capture_output=True, timeout=5, check=False,
        )

    def test_help_needs_no_privileges(self):
        for script in ("install.sh", "uninstall.sh"):
            with self.subTest(script=script):
                result = self.run_script(script, ["--help"])
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn("Usage:", result.stdout)

    def test_invalid_product_ids_rejected_before_authentication(self):
        for value in ("", "123", "12345", "zzzz", "0x1234", "1234\n", "$(id)"):
            with self.subTest(value=value):
                result = self.run_script("install.sh", ["--product", value])
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("Invalid USB product ID", result.stderr)

    def test_xml_and_shell_characters_rejected(self):
        for value in ("", "a" * 129, "a b", "x\ny", "<string>", "a&b", "a'b", 'a"b', "$(id)", "`id`", "가"):
            with self.subTest(value=value):
                result = self.run_script("install.sh", ["--serial", value])
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("Serial must contain", result.stderr)

    def test_supported_serial_boundaries_parse(self):
        for value in ("a", "ABC-123_4.5:6", "a" * 128):
            with self.subTest(value=value):
                result = self.run_script("install.sh", ["--auto", "--product", "ABcd", "--serial", value, "--help"])
                self.assertEqual(result.returncode, 0, result.stderr)

    def test_missing_and_unknown_arguments_fail(self):
        for arguments in (["--product"], ["--serial"], ["--unknown"], ["--auto=1"]):
            with self.subTest(arguments=arguments):
                result = self.run_script("install.sh", arguments)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("GalaxyBridge:", result.stderr)
        self.assertNotEqual(self.run_script("uninstall.sh", ["--force"]).returncode, 0)


class ReleaseLibraries(unittest.TestCase):
    def check_dependency(self, script, name, dependency):
        marker = '\nwhile (( $# )); do' if script == 'install.sh' else '\n[[ $(uname -s) == Darwin ]]'
        prefix = (REPOSITORY / script).read_text().split(marker, 1)[0]
        return subprocess.run(
            ['/bin/bash', '-c', prefix + f'\n{name} "$1"', 'library-check', dependency],
            capture_output=True, text=True, timeout=5, check=False,
        )

    def test_clean_system_libraries_accepted(self):
        for script, owner in [('install.sh', 'installer'), ('scripts/build-release.sh', 'release')]:
            for dependency in ('/usr/lib/libSystem.B.dylib', '/System/Library/Frameworks/IOKit.framework/Versions/A/IOKit'):
                with self.subTest(script=script, dependency=dependency):
                    result = self.check_dependency(script, f'{owner}::binary::dependency::check', dependency)
                    self.assertEqual(result.returncode, 0, result.stderr)

    def test_traversal_and_external_libraries_rejected(self):
        for script, owner in [('install.sh', 'installer'), ('scripts/build-release.sh', 'release')]:
            for dependency in ('/usr/lib/../../tmp/evil.dylib', '/System/Library/../evil.dylib',
                               '/usr/lib/./evil', '/usr/lib/..', '/usr/lib/.',
                               '/usr/lib//evil', '/usr/lib/a\nb', '@rpath/evil.dylib',
                               '/opt/homebrew/lib/libusb.dylib', 'usr/lib/libSystem.B.dylib'):
                with self.subTest(script=script, dependency=dependency):
                    result = self.check_dependency(script, f'{owner}::binary::dependency::check', dependency)
                    self.assertNotEqual(result.returncode, 0)


class InstallerToolchain(unittest.TestCase):
    def check_toolchain(self, selection, discovery):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            selected = directory / 'xcode-select'
            selected.write_text('#!/bin/sh\n' + selection + '\n')
            selected.chmod(0o755)
            discovered = directory / 'xcrun'
            marker = directory / 'discovery-was-called'
            discovered.write_text('#!/bin/sh\ntouch ' + shlex.quote(str(marker)) + '\n' + discovery + '\n')
            discovered.chmod(0o755)
            prefix = (REPOSITORY / 'install.sh').read_text().split('\nwhile (( $# )); do', 1)[0]
            # Substitute only dependency executables in an in-memory test copy;
            # the shipped installer always calls the fixed Apple tool paths.
            prefix = prefix.replace('/usr/bin/xcode-select', shlex.quote(str(selected)))
            prefix = prefix.replace('/usr/bin/xcrun', shlex.quote(str(discovered)))
            result = subprocess.run(['/bin/bash', '-c', prefix + '\ninstaller::toolchain::check'],
                                    capture_output=True, text=True, timeout=5, check=False)
            return result, marker.exists()

    def test_missing_selection_does_not_invoke_tool_discovery(self):
        result, discovered = self.check_toolchain('exit 1', 'exit 1')
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(discovered)
        self.assertIn('Install Apple Command Line Tools once: xcode-select --install', result.stderr)

    def test_missing_tools_fail_with_actionable_message(self):
        for discovery in ('exit 1', "printf '/does-not-exist/otool\\n'"):
            with self.subTest(discovery=discovery):
                result, discovered = self.check_toolchain('exit 0', discovery)
                self.assertNotEqual(result.returncode, 0)
                self.assertTrue(discovered)
                self.assertIn('xcode-select --install', result.stderr)

    def test_existing_executables_are_accepted(self):
        result, discovered = self.check_toolchain('exit 0', "printf '/bin/sh\\n'")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(discovered)


@unittest.skipUnless(__import__('sys').platform == 'darwin', 'macOS filesystem metadata required')
class InstallerPaths(unittest.TestCase):
    def check_path(self, script, name, path):
        # Load only constants/functions, never the installation entry point.
        marker = '\nwhile (( $# )); do' if script == 'install.sh' else '\nif (( $# )); then'
        prefix = (REPOSITORY / script).read_text().split(marker, 1)[0]
        return subprocess.run(
            ['/bin/bash', '-c', prefix + f'\n{name} "$1"', 'path-check', str(path)],
            capture_output=True, text=True, timeout=5, check=False,
        )

    def test_root_directory_is_accepted(self):
        for script, owner in [('install.sh', 'installer'), ('uninstall.sh', 'uninstaller')]:
            result = self.check_path(script, f'{owner}::path::check', '/')
            self.assertEqual(result.returncode, 0, result.stderr)

    def test_symlink_to_trusted_directory_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            link = Path(temporary) / 'redirect'
            link.symlink_to('/', target_is_directory=True)
            for script, owner in [('install.sh', 'installer'), ('uninstall.sh', 'uninstaller')]:
                result = self.check_path(script, f'{owner}::path::check', link)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('Unsafe directory', result.stderr)

    @unittest.skipIf(__import__('os').geteuid() == 0, 'must exercise ordinary-user ownership')
    def test_user_owned_directory_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            for script, owner in [('install.sh', 'installer'), ('uninstall.sh', 'uninstaller')]:
                result = self.check_path(script, f'{owner}::path::check', temporary)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('not root-owned', result.stderr)

    def test_uninstaller_rejects_symlinked_file(self):
        with tempfile.TemporaryDirectory() as temporary:
            link = Path(temporary) / 'redirect'
            link.symlink_to('/etc/passwd')
            result = self.check_path('uninstall.sh', 'uninstaller::file::check', link)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('unsafe installation file', result.stderr)


if __name__ == "__main__":
    unittest.main()
