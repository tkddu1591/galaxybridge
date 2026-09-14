#![cfg(target_os = "macos")]

use galaxybridge::macos::identity::{Account, HOME};

const RECEIPT: &str = "version=1\nuid=60000\ngid=60000\nuser_guid=11111111-1111-4111-8111-111111111111\ngroup_guid=22222222-2222-4222-8222-222222222222\n";

#[test]
fn identity_receipt_cannot_select_root_nobody_or_a_human_account() {
    let account = Account::parse(RECEIPT).unwrap();
    assert_eq!((account.uid, account.gid), (60000, 60000));
    for id in ["0", "501", "59999", "65000", "4294967294", "-2", "invalid"] {
        for field in ["uid", "gid"] {
            assert!(
                Account::parse(
                    &RECEIPT.replace(&format!("{field}=60000"), &format!("{field}={id}"))
                )
                .is_err()
            );
        }
    }
    for text in [
        RECEIPT.replace("version=1", "version=2"),
        format!("{RECEIPT}uid=60001\n"),
        format!("{RECEIPT}unknown=value\n"),
        RECEIPT.replace(
            "user_guid=11111111-1111-4111-8111-111111111111",
            "user_guid=anything",
        ),
        RECEIPT.replace(
            "22222222-2222-4222-8222-222222222222",
            "11111111-1111-4111-8111-111111111111",
        ),
    ] {
        assert!(Account::parse(&text).is_err());
    }
}

#[test]
fn replaced_reenabled_or_shared_directory_accounts_are_rejected() {
    let account = Account::parse(RECEIPT).unwrap();
    let user = format!(
        "UniqueID: 60000\nPrimaryGroupID: 60000\nGeneratedUID: 11111111-1111-4111-8111-111111111111\nAuthenticationAuthority: ;DisabledUser;\nUserShell: /usr/bin/false\nNFSHomeDirectory: {HOME}\ndsAttrTypeNative:IsHidden: 1\nPassword: *\n"
    );
    let group = "PrimaryGroupID: 60000\nGeneratedUID: 22222222-2222-4222-8222-222222222222\nGroupMembership: _galaxybridge\nGroupMembers: 11111111-1111-4111-8111-111111111111\nPassword: *\n";
    account.validate(&user, group).unwrap();
    assert!(
        account
            .validate(&format!("{user}IsHidden: 1\n"), group)
            .is_err()
    );
    for (from, to) in [
        ("UniqueID: 60000", "UniqueID: 60001"),
        ("PrimaryGroupID: 60000", "PrimaryGroupID: 0"),
        (
            "11111111-1111-4111-8111-111111111111",
            "33333333-3333-4333-8333-333333333333",
        ),
        (";DisabledUser;", ";ShadowHash;"),
        ("/usr/bin/false", "/bin/zsh"),
        (HOME, "/Users/example"),
        ("IsHidden: 1", "IsHidden: 0"),
        ("Password: *", "Password: usable"),
    ] {
        assert!(
            account.validate(&user.replace(from, to), group).is_err(),
            "{from}"
        );
    }
    for (from, to) in [
        ("PrimaryGroupID: 60000", "PrimaryGroupID: 60001"),
        (
            "22222222-2222-4222-8222-222222222222",
            "33333333-3333-4333-8333-333333333333",
        ),
        (
            "GroupMembership: _galaxybridge",
            "GroupMembership: _galaxybridge another_user",
        ),
        (
            "GroupMembers: 11111111-1111-4111-8111-111111111111",
            "GroupMembers: 33333333-3333-4333-8333-333333333333",
        ),
    ] {
        assert!(
            account.validate(&user, &group.replace(from, to)).is_err(),
            "{from}"
        );
    }
    assert!(
        account
            .validate(&format!("{user}UniqueID: 60000\n"), group)
            .is_err()
    );
}
