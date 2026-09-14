#!/usr/bin/env python3
"""Adversarial account-lifecycle tests. Uses a stateful directory mock only.

Never invokes dscl, creates a macOS account, changes ownership, or uses sudo.
"""
import json
import hashlib
import os
from pathlib import Path
import shlex
import subprocess
import tempfile
import unittest

REPOSITORY = Path(__file__).resolve().parent.parent
USER_GUID = '11111111-1111-4111-8111-111111111111'
GROUP_GUID = '22222222-2222-4222-8222-222222222222'
RECEIPT = f'version=1\nuid=60000\ngid=60000\nuser_guid={USER_GUID}\ngroup_guid={GROUP_GUID}\n'

MOCK = r'''
import json, pathlib, sys
state_path = pathlib.Path(sys.argv[1])
state = json.loads(state_path.read_text())
tool = pathlib.Path(sys.argv[2]).name
args = sys.argv[3:]
state.setdefault('calls', []).append([tool, *args])
state_path.write_text(json.dumps(state))
def save():
    state_path.write_text(json.dumps(state))
records = state.setdefault('records', {})
if tool == 'dscl':
    assert args[0] == '/Local/Default'
    action = args[1]
    record = args[2]
    if action == '-list':
        if state.get('directory_error'):
            sys.exit(70)
        for key, attributes in sorted(records.items()):
            if key.startswith(record + '/'):
                if len(args) == 4:
                    if args[3] in attributes:
                        print(key.rsplit('/', 1)[1], attributes[args[3]])
                else:
                    print(key.rsplit('/', 1)[1])
    elif action == '-read':
        if record not in records or args[3] not in records[record]:
            sys.exit(1)
        attribute = ('dsAttrTypeNative:' if args[3] == 'IsHidden' else '') + args[3]
        print(attribute + ': ' + records[record][args[3]])
    elif action == '-create':
        attribute, value = args[3:]
        if state.get('fail_attribute') == attribute and record.startswith('/Users/'):
            sys.exit(71)
        records.setdefault(record, {})[attribute] = value
        save()
    elif action == '-delete':
        if state.get('delete_noop') != record:
            del records[record]
        save()
    else:
        raise AssertionError(args)
elif tool == 'dscacheutil':
    kind, attribute, value = args[1], args[3], args[4]
    key = ':'.join((kind, attribute, value))
    if key in state.get('directory_collisions', []):
        print('name: external-directory-collision')
elif tool == 'id':
    attribute = 'UniqueID' if args[0] == '-u' else 'PrimaryGroupID'
    try:
        print(records['/Users/_galaxybridge'][attribute])
    except KeyError:
        sys.exit(1)
elif tool == 'pgrep':
    if state.get('process_query_error'):
        sys.exit(2)
    running = state.get('process_running') or (args[0] == '-U' and state.get('real_uid_process'))
    sys.exit(0 if running else 1)
elif tool == 'launchctl':
    assert args[1] == 'user/60000', args
    if args[0] == 'print':
        # Reproduce the live bug: resolving a user target recreates its context.
        state['domain_present'] = True
        state['process_running'] = True
        state['user_print_recreated_domain'] = True
        save()
        sys.exit(0)
    assert args[0] == 'bootout', args
    if state.get('bootout_error'):
        sys.exit(5)
    if not state.get('bootout_noop'):
        state['process_running'] = False
        state['real_uid_process'] = False
        state['domain_present'] = False
        save()
elif tool == 'mount':
    print(state.get('mount_output', ''))
else:
    raise AssertionError((tool, args))
'''


class IdentityLifecycle(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.directory = Path(self.temporary.name)
        self.state = self.directory / 'state.json'
        self.state.write_text(json.dumps({'records': {}, 'calls': []}))
        self.receipt = self.directory / 'IDENTITY'
        self.receipt.write_text(RECEIPT)
        self.mock = self.directory / 'directory.py'
        self.mock.write_text(MOCK)

    def tearDown(self):
        self.temporary.cleanup()

    def update(self, **values):
        data = json.loads(self.state.read_text())
        data.update(values)
        self.state.write_text(json.dumps(data))

    def run_module(self, commands, extra=(), home_fixture=False, device_mismatch=False):
        script = (REPOSITORY / 'scripts/identity.sh').read_text()
        if home_fixture:
            script = script.replace('readonly identity_data=/private/var/db/io.galaxybridge',
                                    'readonly identity_data=' + shlex.quote(str(self.directory / 'data')))
            script = script.replace('readonly identity_home=/private/var/db/io.galaxybridge/worker',
                                    'readonly identity_home=' + shlex.quote(str(self.directory / 'data/worker')))
            # The test filesystem belongs to the test user. Replace only root
            # metadata guards; the production physical deletion and transaction
            # order below run unchanged on actual temporary files.
            if home_fixture is True:
                script += '\nidentity::home::parent::check() { /usr/bin/cmp -s "$1" "$identity_data/IDENTITY"; }\n'
                script += '\nidentity::home::directory::check() { [[ -d "$identity_home" && ! -L "$identity_home" ]]; }\n'
        if device_mismatch:
            stat = self.directory / 'stat.py'
            home = self.directory / 'data/worker'
            stat.write_text('#!' + os.sys.executable + '\nimport os, sys\n'
                            + 'if sys.argv[1:] == ' + repr(['-f', '%d', str(home)]) + ': print(987654321)\n'
                            + 'else: os.execv("/usr/bin/stat", ["stat", *sys.argv[1:]])\n')
            stat.chmod(0o755)
            script = script.replace('/usr/bin/stat', shlex.quote(str(stat)))
        # Override the one external-command boundary after loading functions.
        # Even an unintended command sent by the module fails inside the mock.
        script += '\nidentity::command::run() { if [[ "$1" == /bin/sleep ]]; then SECONDS=$((SECONDS + 21)); return 0; fi; ' + shlex.quote(os.sys.executable) + ' ' + shlex.quote(str(self.mock)) + ' ' + shlex.quote(str(self.state)) + ' "$@"; }\n'
        script += commands
        return subprocess.run(['/bin/bash', '-euo', 'pipefail', '-c', script, 'identity-test', str(self.receipt), *map(str, extra)],
                              capture_output=True, text=True, timeout=15, check=False)

    def records(self):
        return json.loads(self.state.read_text())['records']

    def test_numeric_user_identity_is_assigned_last(self):
        result = self.run_module('identity::account::create "$1"')
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = json.loads(self.state.read_text())['calls']
        user_creates = [call for call in calls if call[:4] == ['dscl', '/Local/Default', '-create', '/Users/_galaxybridge']]
        self.assertEqual(user_creates[0][4], 'GeneratedUID')
        self.assertEqual(user_creates[-1][4], 'UniqueID')
        before_uid = {call[4]: call[5] for call in user_creates[:-1]}
        self.assertEqual(before_uid['AuthenticationAuthority'], ';DisabledUser;')
        self.assertEqual(before_uid['Password'], '*')
        self.assertEqual(before_uid['UserShell'], '/usr/bin/false')
        self.assertEqual(before_uid['NFSHomeDirectory'], '/private/var/db/io.galaxybridge/worker')
        self.assertEqual(before_uid['IsHidden'], '1')
        self.assertEqual(before_uid['PrimaryGroupID'], '60000')

    def test_preexisting_name_never_modified(self):
        for kind in ('/Users', '/Groups'):
            with self.subTest(kind=kind):
                existing = {f'{kind}/_galaxybridge': {'GeneratedUID': 'UNRELATED'}}
                self.update(records=existing, calls=[])
                result = self.run_module('identity::account::create "$1"')
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(self.records(), existing)
                self.assertNotIn('-create', str(json.loads(self.state.read_text())['calls']))

    def test_remote_name_collision_and_directory_failure_fail_closed(self):
        for values in ({'directory_collisions': ['user:name:_galaxybridge']},
                       {'directory_collisions': [], 'directory_error': True}):
            with self.subTest(values=values):
                self.update(**values)
                result = self.run_module('identity::account::create "$1"')
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(self.records(), {})

    def test_local_or_directory_numeric_collision_selects_another_id(self):
        self.receipt.unlink()
        self.update(records={'/Users/other': {'UniqueID': '60000'}},
                    directory_collisions=['group:gid:60001'])
        result = self.run_module('identity::receipt::create "$1"')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('uid=60002\ngid=60002\n', self.receipt.read_text())
        self.assertEqual(self.receipt.stat().st_mode & 0o777, 0o600)

    def test_partial_creation_rollback_uses_guid_ownership(self):
        self.update(fail_attribute='UserShell')
        self.receipt.unlink()
        result = self.run_module('identity::receipt::create "$1"; if identity::account::create "$1"; then exit 90; fi\nidentity::account::delete "$1" 1')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.records(), {})
        calls = json.loads(self.state.read_text())['calls']
        self.assertFalse(any(call[:5] == ['dscl', '/Local/Default', '-create', '/Users/_galaxybridge', 'UniqueID'] for call in calls))

    def test_replaced_guid_prevents_deletion_of_either_record(self):
        self.assertEqual(self.run_module('identity::account::create "$1"').returncode, 0)
        records = self.records()
        records['/Groups/_galaxybridge']['GeneratedUID'] = 'AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA'
        self.update(records=records)
        result = self.run_module('identity::account::delete "$1"')
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.records(), records)

    def test_changed_login_attributes_block_uninstall(self):
        self.assertEqual(self.run_module('identity::account::create "$1"').returncode, 0)
        records = self.records()
        records['/Users/_galaxybridge']['UserShell'] = '/bin/bash'
        self.update(records=records)
        result = self.run_module('identity::account::delete "$1"')
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.records(), records)

    def test_later_numeric_alias_is_not_an_exclusive_worker_identity(self):
        self.assertEqual(self.run_module('identity::account::create "$1"').returncode, 0)
        records = self.records()
        records['/Users/another-service'] = {'UniqueID': '60000'}
        self.update(records=records)
        result = self.run_module('identity::account::check "$1"')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('not exclusive', result.stderr)
        self.assertEqual(self.records(), records)

    def test_running_worker_prevents_identity_reuse(self):
        self.assertEqual(self.run_module('identity::account::create "$1"').returncode, 0)
        self.update(process_running=True)
        before = self.records()
        result = self.run_module('identity::account::delete "$1"')
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.records(), before)

    def test_sidecar_shutdown_targets_only_owned_user_domain(self):
        self.assertEqual(self.run_module('identity::account::create "$1"').returncode, 0)
        self.update(process_running=True)
        before = self.records()
        result = self.run_module('identity::session::stop "$1"')
        self.assertEqual(result.returncode, 0, result.stderr)
        state = json.loads(self.state.read_text())
        self.assertFalse(state['process_running'])
        self.assertEqual(self.records(), before, 'Stopping the domain must not remove account records')
        actions = [call for call in state['calls'] if call[:2] == ['launchctl', 'bootout']]
        self.assertEqual(actions, [['launchctl', 'bootout', 'user/60000']])

    def test_mismatched_guid_prevents_user_domain_action(self):
        self.assertEqual(self.run_module('identity::account::create "$1"').returncode, 0)
        records = self.records()
        records['/Users/_galaxybridge']['GeneratedUID'] = 'AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA'
        self.update(records=records, process_running=True)
        result = self.run_module('identity::session::stop "$1"')
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(any(call[:2] == ['launchctl', 'bootout'] for call in json.loads(self.state.read_text())['calls']))
        self.assertEqual(self.records(), records)

    def test_numeric_alias_prevents_user_domain_action(self):
        self.assertEqual(self.run_module('identity::account::create "$1"').returncode, 0)
        records = self.records()
        records['/Users/another-account'] = {'UniqueID': '60000'}
        self.update(records=records, process_running=True)
        result = self.run_module('identity::session::stop "$1"')
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(any(call[:2] == ['launchctl', 'bootout'] for call in json.loads(self.state.read_text())['calls']))

    def test_failed_or_noop_bootout_retains_identity_and_receipt(self):
        self.assertEqual(self.run_module('identity::account::create "$1"').returncode, 0)
        before = self.records()
        for controls in ({'bootout_error': True, 'bootout_noop': False},
                         {'bootout_error': False, 'bootout_noop': True}):
            with self.subTest(controls=controls):
                self.update(process_running=True, **controls)
                result = self.run_module('identity::session::stop "$1" && identity::installation::delete "$1"')
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(self.records(), before)
                self.assertTrue(self.receipt.exists())

    def test_process_query_error_blocks_domain_action(self):
        self.assertEqual(self.run_module('identity::account::create "$1"').returncode, 0)
        self.update(process_query_error=True)
        result = self.run_module('identity::session::stop "$1"')
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(any(call[:2] == ['launchctl', 'bootout'] for call in json.loads(self.state.read_text())['calls']))

    def test_idle_registered_domain_is_removed_even_without_processes(self):
        self.assertEqual(self.run_module('identity::account::create "$1"').returncode, 0)
        self.update(domain_present=True, process_running=False)
        result = self.run_module('identity::session::stop "$1"')
        self.assertEqual(result.returncode, 0, result.stderr)
        state = json.loads(self.state.read_text())
        self.assertFalse(state['domain_present'])
        self.assertIn(['launchctl', 'bootout', 'user/60000'], state['calls'])

    def test_shutdown_never_queries_a_user_domain_that_would_recreate_it(self):
        self.assertEqual(self.run_module('identity::account::create "$1"').returncode, 0)
        self.update(domain_present=True, process_running=True)
        result = self.run_module('identity::session::stop "$1"; identity::session::proof::check')
        self.assertEqual(result.returncode, 0, result.stderr)
        state = json.loads(self.state.read_text())
        self.assertFalse(state.get('user_print_recreated_domain', False))
        self.assertFalse(state['domain_present'])
        self.assertFalse(any(call[:2] == ['launchctl', 'print'] for call in state['calls']))

    def test_persisted_orphan_without_live_proof_is_retained(self):
        result = self.run_module('identity::account::delete "$1" 1')
        self.assertNotEqual(result.returncode, 0)
        self.assertTrue(self.receipt.exists())
        self.assertFalse(any(call[:2] == ['launchctl', 'bootout'] for call in json.loads(self.state.read_text())['calls']))

    def test_account_deletion_itself_requires_teardown_proof(self):
        self.assertEqual(self.run_module('identity::account::create "$1"').returncode, 0)
        before = self.records()
        result = self.run_module('identity::account::delete "$1"')
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.records(), before)

    def test_teardown_token_rejects_changed_uid_gid_or_guids(self):
        self.assertEqual(self.run_module('identity::account::create "$1"').returncode, 0)
        for change in ('identity_uid=60001', 'identity_gid=60001',
                       'identity_user_guid=AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA',
                       'identity_group_guid=AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA'):
            with self.subTest(change=change):
                result = self.run_module('identity::session::stop "$1"; ' + change + '; identity::session::proof::check')
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('No matching in-process', result.stderr)

    def test_receipt_created_in_another_shell_is_not_fresh_provenance(self):
        self.receipt.unlink()
        self.assertEqual(self.run_module('identity::receipt::create "$1"').returncode, 0)
        result = self.run_module('identity::receipt::load "$1"; identity::session::proof::check 1')
        self.assertNotEqual(result.returncode, 0)

    def test_fresh_receipt_with_home_started_cannot_skip_teardown(self):
        self.receipt.unlink()
        result = self.run_module('identity::receipt::create "$1"; identity_home_created=1; identity::session::proof::check 1')
        self.assertNotEqual(result.returncode, 0)

    def test_failed_install_with_home_runs_scoped_teardown_before_cleanup(self):
        data = self.directory / 'data'
        home = data / 'worker'
        home.mkdir(parents=True)
        (data / 'IDENTITY').write_text(RECEIPT)
        (home / 'container-state').write_text('Partial installation state\n')
        self.assertEqual(self.run_module('identity::account::create "$1"', home_fixture=True).returncode, 0)
        self.update(domain_present=True, process_running=True)
        result = self.run_module('identity_home_created=1; identity_home_directory_created=1; identity::installation::delete "$1" 1', home_fixture=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(data.exists())
        self.assertEqual(self.records(), {})
        state = json.loads(self.state.read_text())
        actions = state['calls']
        bootout = actions.index(['launchctl', 'bootout', 'user/60000'])
        first_delete = next(i for i, call in enumerate(actions) if call[:3] == ['dscl', '/Local/Default', '-delete'])
        self.assertLess(bootout, first_delete)
        self.assertFalse(state.get('user_print_recreated_domain', False))

    def test_real_uid_only_process_is_not_mistaken_for_quiescence(self):
        self.assertEqual(self.run_module('identity::account::create "$1"').returncode, 0)
        self.update(real_uid_process=True)
        result = self.run_module('identity::receipt::load "$1"; identity::process::check')
        self.assertNotEqual(result.returncode, 0)
        result = self.run_module('identity::session::stop "$1"')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(json.loads(self.state.read_text())['real_uid_process'])

    def test_same_invocation_removal_retry_preserves_teardown_proof(self):
        self.assertEqual(self.run_module('identity::account::create "$1"').returncode, 0)
        result = self.run_module('identity::session::stop "$1"; identity::command::run /usr/bin/dscl /Local/Default -delete /Users/_galaxybridge; identity::account::delete "$1"; identity::account::delete "$1"')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.records(), {})

    def test_success_status_without_record_removal_is_not_accepted(self):
        self.assertEqual(self.run_module('identity::account::create "$1"').returncode, 0)
        self.update(delete_noop='/Users/_galaxybridge')
        records = self.records()
        result = self.run_module('identity::session::stop "$1"; identity::account::delete "$1"')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('record remains after deletion', result.stderr)
        self.assertEqual(self.records(), records)
        self.assertTrue(self.receipt.exists())

    def test_receipt_parser_rejects_injection_and_ambiguous_records(self):
        invalid = [RECEIPT + 'uid=60001\n', RECEIPT + 'extra', RECEIPT.rstrip('\n'),
                   RECEIPT.replace('uid=60000', 'uid=0'), RECEIPT.replace('gid=60000', 'gid=60001'),
                   RECEIPT.replace('60000', '$(id)'), RECEIPT.replace('version=1', 'version=1\x00'),
                   RECEIPT.replace(USER_GUID, GROUP_GUID), RECEIPT.replace('version=1', 'version=2')]
        for content in invalid:
            with self.subTest(content=repr(content)):
                self.receipt.write_bytes(content.encode())
                result = self.run_module('identity::receipt::load "$1"')
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(self.records(), {})

    def test_home_purge_unlinks_symlinks_and_hardlinks_without_touching_outside(self):
        outside = self.directory / 'outside'
        outside.mkdir()
        protected = outside / 'keep.txt'
        protected.write_bytes(b'Outside data must remain unchanged\n')
        protected.chmod(0o640)
        before_hash = hashlib.sha256(protected.read_bytes()).digest()
        before_mode = protected.stat().st_mode
        home = self.directory / 'home'
        home.mkdir()
        (home / 'directory-link').symlink_to(outside, target_is_directory=True)
        (home / 'file-link').symlink_to(protected)
        os.link(protected, home / 'hard-link')
        (home / 'own-file').write_text('Owned container state\n')
        result = self.run_module('identity::home::tree::delete "$2"', [home])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(home.exists())
        self.assertEqual(hashlib.sha256(protected.read_bytes()).digest(), before_hash)
        self.assertEqual(protected.stat().st_mode, before_mode)
        self.assertEqual(protected.stat().st_nlink, 1)

    @unittest.skipUnless(os.sys.platform == 'darwin', 'macOS immutable flags required')
    def test_home_purge_failure_retains_account_and_receipts(self):
        data = self.directory / 'data'
        home = data / 'worker'
        home.mkdir(parents=True)
        (data / 'IDENTITY').write_text(RECEIPT)
        protected = home / 'immutable'
        protected.write_text('Cannot be removed until an administrator reviews it\n')
        subprocess.run(['/usr/bin/chflags', 'uchg', str(protected)], check=True)
        try:
            created = self.run_module('identity::account::create "$1"', home_fixture=True)
            self.assertEqual(created.returncode, 0, created.stderr)
            before = self.records()
            result = self.run_module('identity::session::stop "$1"; identity::installation::delete "$1"', home_fixture=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(self.records(), before)
            self.assertTrue(self.receipt.exists())
            self.assertEqual((data / 'IDENTITY').read_text(), RECEIPT)
            self.assertTrue(protected.exists())
        finally:
            subprocess.run(['/usr/bin/chflags', '0', str(protected)], check=False)

    def test_replaced_guid_blocks_home_purge_before_any_data_is_deleted(self):
        data = self.directory / 'data'
        home = data / 'worker'
        home.mkdir(parents=True)
        (data / 'IDENTITY').write_text(RECEIPT)
        saved = home / 'container-state'
        saved.write_text('Must survive ownership mismatch\n')
        self.assertEqual(self.run_module('identity::account::create "$1"', home_fixture=True).returncode, 0)
        records = self.records()
        records['/Users/_galaxybridge']['GeneratedUID'] = 'AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA'
        self.update(records=records)
        result = self.run_module('identity::session::stop "$1"; identity::installation::delete "$1"', home_fixture=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(saved.read_text(), 'Must survive ownership mismatch\n')
        self.assertEqual(self.records(), records)

    def test_missing_user_record_with_existing_home_is_not_a_cleanup_retry(self):
        data = self.directory / 'data'
        home = data / 'worker'
        home.mkdir(parents=True)
        (data / 'IDENTITY').write_text(RECEIPT)
        saved = home / 'container-state'
        saved.write_text('Retain data when account ownership is missing\n')
        self.assertEqual(self.run_module('identity::account::create "$1"', home_fixture=True).returncode, 0)
        records = self.records()
        del records['/Users/_galaxybridge']
        self.update(records=records)
        result = self.run_module('identity::session::stop "$1"; identity::installation::delete "$1"', home_fixture=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertTrue(saved.exists())
        self.assertEqual(self.records(), records)

    @unittest.skipUnless(os.sys.platform == 'darwin', 'macOS stat metadata required')
    def test_top_level_home_mount_is_refused_even_on_same_device(self):
        home = self.directory / 'data/worker'
        home.mkdir(parents=True, mode=0o700)
        self.update(mount_output=f'fixture-volume on {home} (apfs, local)')
        result = self.run_module(f'identity_uid={os.getuid()}; identity_gid={os.getgid()}; identity::home::directory::check', home_fixture='paths')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('mount point', result.stderr)
        self.assertTrue(home.exists())

    @unittest.skipUnless(os.sys.platform == 'darwin', 'macOS stat metadata required')
    def test_top_level_home_device_differs_from_parent(self):
        home = self.directory / 'data/worker'
        home.mkdir(parents=True, mode=0o700)
        result = self.run_module(f'identity_uid={os.getuid()}; identity_gid={os.getgid()}; identity::home::directory::check',
                                 home_fixture='paths', device_mismatch=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('different filesystem', result.stderr)
        self.assertTrue(home.exists())


if __name__ == '__main__':
    unittest.main()
