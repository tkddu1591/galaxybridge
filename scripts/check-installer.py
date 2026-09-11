#!/usr/bin/env python3
"""Non-privileged installer input checks. Never runs installation or sudo."""
from pathlib import Path
import subprocess
import shlex
import tempfile
import unittest
import sys
import plistlib
import os

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
        for option in ('product', 'vendor'):
            for value in ("", "123", "12345", "zzzz", "0x1234", "1234\n", "$(id)"):
                with self.subTest(option=option, value=value):
                    result = self.run_script("install.sh", [f'--{option}', value])
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(f"Invalid USB {option} ID", result.stderr)

    def test_xml_and_shell_characters_rejected(self):
        for value in ("", "a" * 129, "a b", "x\ny", "<string>", "a&b", "a'b", 'a"b', "$(id)", "`id`", "가"):
            with self.subTest(value=value):
                result = self.run_script("install.sh", ["--serial", value])
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("Serial must contain", result.stderr)

    def test_supported_serial_boundaries_parse(self):
        for value in ("a", "ABC-123_4.5:6", "a" * 128):
            with self.subTest(value=value):
                result = self.run_script("install.sh", ["--auto", "--vendor", "18d1", "--product", "ABcd", "--serial", value, "--help"])
                self.assertEqual(result.returncode, 0, result.stderr)

    def test_missing_and_unknown_arguments_fail(self):
        for arguments in (["--vendor"], ["--product"], ["--serial"], ["--unknown"], ["--auto=1"]):
            with self.subTest(arguments=arguments):
                result = self.run_script("install.sh", arguments)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("GalaxyBridge:", result.stderr)
        self.assertNotEqual(self.run_script("uninstall.sh", ["--force"]).returncode, 0)


class SourceInvoker(unittest.TestCase):
    def test_root_handoff_and_unprivileged_spoofing(self):
        prefix = (REPOSITORY / 'install.sh').read_text().split('\nwhile (( $# )); do', 1)[0]
        cases = [('0', '501', '501'), ('501', '60000', '501'), ('0', '', '0'), ('502', '', '502')]
        for effective, sudo_uid, expected in cases:
            with self.subTest(effective=effective, sudo_uid=sudo_uid):
                result = subprocess.run(['/bin/bash', '-c', prefix + '\ninstaller::source::uid::get "$1" "$2"',
                                         'uid-test', effective, sudo_uid], capture_output=True, text=True, timeout=5)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout, expected)
        for invalid in ('-1', '0501', '4294967295', '$(id)', '501\n'):
            result = subprocess.run(['/bin/bash', '-c', prefix + '\ninstaller::source::uid::get 0 "$1"',
                                     'uid-test', invalid], capture_output=True, text=True, timeout=5)
            self.assertNotEqual(result.returncode, 0)


@unittest.skipUnless(sys.platform == 'darwin' and os.geteuid() != 0, 'ordinary macOS user required')
class SourceBootstrap(unittest.TestCase):
    def setUp(self):
        # Exercise the root-owned sticky ancestor exception on the real OS.
        self.temporary = tempfile.TemporaryDirectory(prefix='galaxybridge-source-', dir='/private/tmp')
        self.bundle = Path(self.temporary.name)
        self.tool_marker = self.bundle / 'tool-dispatch-was-called'
        self.sudo_marker = self.bundle / 'sudo-was-called'
        self.sudo_arguments = self.bundle / 'sudo-arguments'
        self.script = (REPOSITORY / 'install.sh').read_text()
        for name in ('SHA256SUMS', 'bin/galaxybridge', 'uninstall.sh', 'libexec/identity.sh',
                     'libexec/USBWorker.app/Contents/Info.plist',
                     'libexec/USBWorker.app/Contents/MacOS/galaxybridge-usb',
                     'libexec/USBWorker.app/Contents/_CodeSignature/CodeResources'):
            file = self.bundle / name
            file.parent.mkdir(parents=True, exist_ok=True)
            file.write_text('Source validation fixture; never executed or installed\n')
        select = self.bundle / 'mock-xcode-select'
        select.write_text('#!/bin/sh\ntouch ' + shlex.quote(str(self.tool_marker)) + '\nprintf "/System\\n"\n')
        select.chmod(0o755)
        discovery = self.bundle / 'mock-xcrun'
        discovery.write_text('#!/bin/sh\nprintf "/bin/sh\\n"\n')
        discovery.chmod(0o755)
        sudo = self.bundle / 'mock-sudo'
        sudo.write_text('#!/bin/sh\ntouch ' + shlex.quote(str(self.sudo_marker))
                        + '\nprintf "%s\\n" "$@" > ' + shlex.quote(str(self.sudo_arguments)) + '\nexit 0\n')
        sudo.chmod(0o755)
        self.script = self.script.replace('/usr/bin/xcode-select', shlex.quote(str(select)))
        self.script = self.script.replace('/usr/bin/xcrun', shlex.quote(str(discovery)))
        self.script = self.script.replace('/usr/bin/sudo', shlex.quote(str(sudo)))
        (self.bundle / 'install.sh').write_text(self.script)

    def tearDown(self):
        self.temporary.cleanup()

    def run_installer(self):
        return subprocess.run(['/bin/bash', str(self.bundle / 'install.sh')], capture_output=True,
                              text=True, timeout=15, check=False)

    def assert_blocked_before_tools_and_sudo(self):
        result = self.run_installer()
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.tool_marker.exists())
        self.assertFalse(self.sudo_marker.exists())
        self.assertIn('Unsafe release source', result.stderr)
        self.assertIn('private folder', result.stderr)
        return result

    def test_private_bundle_under_sticky_tmp_reaches_auth_with_correct_uid(self):
        result = self.run_installer()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(self.tool_marker.exists())
        self.assertTrue(self.sudo_marker.exists())
        self.assertIn(f'SUDO_UID={os.geteuid()}\n', self.sudo_arguments.read_text())

    def test_group_writable_copied_module_blocks_before_authentication(self):
        (self.bundle / 'libexec/identity.sh').chmod(0o664)
        self.assert_blocked_before_tools_and_sudo()

    def test_write_acl_on_installer_blocks_before_any_tool_dispatch(self):
        subprocess.run(['/bin/chmod', '+a', 'everyone allow write', str(self.bundle / 'install.sh')], check=True)
        self.assert_blocked_before_tools_and_sudo()

    def test_writable_worker_app_ancestor_blocks_before_authentication(self):
        (self.bundle / 'libexec/USBWorker.app/Contents').chmod(0o775)
        self.assert_blocked_before_tools_and_sudo()

    def test_acl_on_manifest_blocks_even_when_posix_mode_is_private(self):
        file = self.bundle / 'SHA256SUMS'
        file.chmod(0o600)
        subprocess.run(['/bin/chmod', '+a', 'everyone allow write', str(file)], check=True)
        self.assert_blocked_before_tools_and_sudo()

    def test_symlinked_selected_file_is_rejected(self):
        module = self.bundle / 'libexec/identity.sh'
        module.unlink()
        module.symlink_to(self.bundle / 'uninstall.sh')
        self.assert_blocked_before_tools_and_sudo()

    def test_foreign_owner_is_rejected_without_chown_or_root(self):
        foreign = self.bundle / 'bin/galaxybridge'
        mock = self.bundle / 'mock-stat.py'
        mock.write_text('#!' + sys.executable + '\nimport os, sys\n'
                        + 'if sys.argv[1:] == ' + repr(['-f', '%u', str(foreign)]) + ': print(60001)\n'
                        + 'else: os.execv("/usr/bin/stat", ["stat", *sys.argv[1:]])\n')
        mock.chmod(0o755)
        (self.bundle / 'install.sh').write_text(self.script.replace('/usr/bin/stat', shlex.quote(str(mock))))
        result = self.assert_blocked_before_tools_and_sudo()
        self.assertIn('owned by another user', result.stderr)

    def test_shared_tmp_itself_is_not_an_acceptable_bundle(self):
        prefix = (REPOSITORY / 'install.sh').read_text().split('\nwhile (( $# )); do', 1)[0]
        result = subprocess.run(['/bin/bash', '-c', prefix + '\ninstaller::source::directory::check /private/tmp "$1"',
                                 'shared-source-test', str(os.geteuid())], capture_output=True, text=True, timeout=5)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('writable by another user', result.stderr)


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


@unittest.skipUnless(sys.platform == 'darwin', 'macOS trusted system paths required')
class InstallerToolchain(unittest.TestCase):
    def check_toolchain(self, selection, discovery):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            selected = directory / 'xcode-select'
            selection = selection.replace('USER_DIRECTORY', shlex.quote(str(directory)))
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
                result, discovered = self.check_toolchain("printf '/System\\n'", discovery)
                self.assertNotEqual(result.returncode, 0)
                self.assertTrue(discovered)
                self.assertIn('xcode-select --install', result.stderr)

    def test_existing_executables_are_accepted(self):
        result, discovered = self.check_toolchain("printf '/System\\n'", "printf '/bin/sh\\n'")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(discovered)

    def test_user_writable_selected_toolchain_is_never_executed(self):
        result, discovered = self.check_toolchain("printf '%s\\n' USER_DIRECTORY", "printf '/bin/sh\\n'")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(discovered)
        self.assertIn('not root-owned', result.stderr)


@unittest.skipUnless(sys.platform == 'darwin', 'macOS ACL and extended attributes required')
class InstallerMetadata(unittest.TestCase):
    def test_copied_write_acl_and_flags_removed_but_quarantine_preserved(self):
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary) / 'source'
            target = Path(temporary) / 'target'
            source.write_text('Non-executable security test fixture\n')
            subprocess.run(['/bin/chmod', '+a', 'everyone allow write', str(source)], check=True)
            quarantine = '0083;00000000;GalaxyBridgeTests;'
            subprocess.run(['/usr/bin/xattr', '-w', 'com.apple.quarantine', quarantine, str(source)], check=True)
            subprocess.run(['/usr/bin/install', '-m', '755', str(source), str(target)], check=True)
            before = subprocess.check_output(['/bin/ls', '-le', str(target)], text=True)
            self.assertIn('everyone allow write', before, 'Fixture must reproduce macOS ACL preservation')
            copied_quarantine = subprocess.check_output(['/usr/bin/xattr', '-p', 'com.apple.quarantine', str(target)], text=True).strip()
            subprocess.run(['/usr/bin/chflags', 'uchg,hidden', str(target)], check=True)
            prefix = (REPOSITORY / 'install.sh').read_text().split('\nwhile (( $# )); do', 1)[0]
            try:
                result = subprocess.run(['/bin/bash', '-c', prefix + '\ninstaller::file::metadata::clear "$1" 755',
                                         'metadata-test', str(target)], capture_output=True, text=True, timeout=5)
                self.assertEqual(result.returncode, 0, result.stderr)
                after = subprocess.check_output(['/bin/ls', '-le', str(target)], text=True)
                self.assertNotIn(' allow ', after)
                self.assertEqual(subprocess.check_output(['/usr/bin/stat', '-f', '%f', str(target)], text=True).strip(), '0')
                self.assertEqual(subprocess.check_output(['/usr/bin/xattr', '-p', 'com.apple.quarantine', str(target)], text=True).strip(), copied_quarantine)
            finally:
                subprocess.run(['/usr/bin/chflags', '0', str(target)], check=False)

    def test_unsafe_installed_uninstaller_is_rejected_before_sudo(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            installed = directory / 'unsafe-installation'
            installed.mkdir()
            (installed / 'uninstall.sh').write_text('#!/bin/sh\nexit 99\n')
            sudo_marker = directory / 'sudo-was-called'
            fake_sudo = directory / 'sudo'
            fake_sudo.write_text('#!/bin/sh\ntouch ' + shlex.quote(str(sudo_marker)) + '\nexit 99\n')
            fake_sudo.chmod(0o755)
            script = (REPOSITORY / 'uninstall.sh').read_text()
            script = script.replace('readonly destination=/Library/PrivilegedHelperTools/io.galaxybridge',
                                    'readonly destination=' + shlex.quote(str(installed)))
            script = script.replace('/usr/bin/sudo', shlex.quote(str(fake_sudo)))
            result = subprocess.run(['/bin/bash', '-c', script], capture_output=True, text=True, timeout=5)
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse(sudo_marker.exists())
            self.assertIn('not root-owned', result.stderr)


@unittest.skipUnless(sys.platform == 'darwin', 'Apple plist parser required')
class WorkerEntitlements(unittest.TestCase):
    def check_entitlements(self, values):
        with tempfile.TemporaryDirectory() as temporary:
            file = Path(temporary) / 'entitlements.plist'
            file.write_bytes(plistlib.dumps(values))
            prefix = (REPOSITORY / 'install.sh').read_text().split('\nwhile (( $# )); do', 1)[0]
            return subprocess.run(['/bin/bash', '-c', prefix + '\ninstaller::worker::entitlements::check "$1"',
                                   'entitlement-test', str(file)], capture_output=True, text=True, timeout=5)

    def test_only_two_boolean_permissions_are_accepted(self):
        permissions = {'com.apple.security.app-sandbox': True, 'com.apple.security.device.usb': True}
        self.assertEqual(self.check_entitlements(permissions).returncode, 0)
        variants = [
            {**permissions, 'com.apple.security.network.client': True},
            {**permissions, 'com.apple.security.files.user-selected.read-write': True},
            {**permissions, 'com.apple.security.app-sandbox': 1},
            {**permissions, 'com.apple.security.device.usb': 'true'},
            {**permissions, 'com.apple.security.app-sandbox': False},
            {'com.apple.security.app-sandbox': True},
        ]
        for values in variants:
            with self.subTest(values=values):
                self.assertNotEqual(self.check_entitlements(values).returncode, 0)

    def test_app_directory_quarantine_is_carried_to_staging(self):
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary) / 'source.app'
            target = Path(temporary) / 'target.app'
            source.mkdir()
            target.mkdir()
            quarantine = '0083;00000000;GalaxyBridgeTests;'
            subprocess.run(['/usr/bin/xattr', '-w', 'com.apple.quarantine', quarantine, str(source)], check=True)
            prefix = (REPOSITORY / 'install.sh').read_text().split('\nwhile (( $# )); do', 1)[0]
            result = subprocess.run(['/bin/bash', '-c', prefix + '\ninstaller::file::quarantine::copy "$1" "$2"',
                                     'quarantine-test', str(source), str(target)], capture_output=True, text=True, timeout=5)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(subprocess.check_output(['/usr/bin/xattr', '-p', 'com.apple.quarantine', str(target)], text=True).strip(), quarantine)


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
