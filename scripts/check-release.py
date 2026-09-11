#!/usr/bin/env python3
"""Exercise packaging with a fake Cargo build and a real minimal arm64 Mach-O."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest

REPOSITORY = Path(__file__).resolve().parent.parent


@unittest.skipUnless(sys.platform == 'darwin', 'macOS Mach-O tools required')
class ReleaseOutput(unittest.TestCase):
    def test_target_override_packages_fresh_output(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            source = root / 'source'
            (source / 'scripts').mkdir(parents=True)
            shutil.copyfile(REPOSITORY / 'scripts/build-release.sh', source / 'scripts/build-release.sh')
            for name in ('README.md', 'README.ko.md', 'LICENSE', 'SECURITY.md', 'install.sh', 'uninstall.sh'):
                (source / name).write_text('Packaging test fixture\n')
            (source / 'docs').mkdir()
            (source / 'docs/architecture.md').write_text('Offline documentation fixture\n')
            # A stale fixed-target artifact must never be selected.
            stale = source / 'target/aarch64-apple-darwin/release/galaxybridge'
            stale.parent.mkdir(parents=True)
            stale.write_bytes(b'STALE ARTIFACT: not the binary just built')
            c_source = root / 'fixture.c'
            c_source.write_text('int main(void) { return 0; }\n')
            binary = root / 'fresh-binary'
            subprocess.run(['/usr/bin/xcrun', 'clang', '-target', 'arm64-apple-macos13.3',
                            str(c_source), '-o', str(binary)], check=True, capture_output=True)
            commands = root / 'commands'
            commands.mkdir()
            python = sys.executable
            cargo = commands / 'cargo'
            cargo.write_text(f'#!{python}\n' + '''import json, os, pathlib, shutil, sys
root = pathlib.Path(os.environ['GALAXYBRIDGE_TEST_ROOT'])
if sys.argv[1] == 'build':
    arguments = sys.argv[2:]
    output = pathlib.Path(arguments[arguments.index('--target-dir') + 1]) if '--target-dir' in arguments else pathlib.Path(os.environ['CARGO_TARGET_DIR'])
    destination = output / 'aarch64-apple-darwin/release/galaxybridge'
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(os.environ['GALAXYBRIDGE_TEST_BINARY'], destination)
    (root / 'observed-output.json').write_text(json.dumps(str(output)))
elif sys.argv[1] == 'metadata':
    print(json.dumps({'resolve': {'root': 'fixture', 'nodes': [{'id': 'fixture'}]}, 'packages': [{'id': 'fixture', 'name': 'galaxybridge', 'version': '0.0.0-test'}]}))
elif sys.argv[1] == '--version':
    print('cargo test-fixture')
else:
    sys.exit(2)
''')
            cargo.chmod(0o755)
            rustc = commands / 'rustc'
            rustc.write_text('#!/bin/sh\nprintf "rustc test-fixture\\n"\n')
            rustc.chmod(0o755)
            environment = dict(os.environ, PATH=f'{commands}:{os.environ["PATH"]}',
                               CARGO_TARGET_DIR=str(root / 'inherited-target'),
                               GALAXYBRIDGE_TEST_ROOT=str(root),
                               GALAXYBRIDGE_TEST_BINARY=str(binary))
            result = subprocess.run(['/bin/bash', str(source / 'scripts/build-release.sh')],
                                    env=environment, capture_output=True, text=True, timeout=30)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(json.loads((root / 'observed-output.json').read_text()), str(source / '.build/cargo'))
            archive = source / 'dist/galaxybridge-0.0.0-test-macos-arm64.tar.gz'
            with tarfile.open(archive) as bundle:
                packaged = bundle.extractfile('galaxybridge-0.0.0-test-macos-arm64/bin/galaxybridge').read()
                self.assertEqual(hashlib.sha256(packaged).digest(), hashlib.sha256(binary.read_bytes()).digest())
                manifest = bundle.extractfile('galaxybridge-0.0.0-test-macos-arm64/SHA256SUMS').read().decode()
                self.assertIn(f'{hashlib.sha256(packaged).hexdigest()}  bin/galaxybridge', manifest)
                self.assertIn('  README.ko.md\n', manifest)
                self.assertIn('  docs/architecture.md\n', manifest)
            # Existing release artifacts must not be silently replaced.
            rerun = subprocess.run(['/bin/bash', str(source / 'scripts/build-release.sh')],
                                   env=environment, capture_output=True, text=True, timeout=30)
            self.assertNotEqual(rerun.returncode, 0)
            self.assertIn('Refusing to overwrite release', rerun.stderr)


if __name__ == '__main__':
    unittest.main()
