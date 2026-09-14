//! Validated installer-owned identity for the unprivileged USB worker.
use super::{access::PrivateFile, command};
use crate::Result;
use std::{
    collections::BTreeMap,
    ffi::{CStr, OsString},
    fs::OpenOptions,
    io::Read,
    os::unix::{ffi::OsStringExt, fs::OpenOptionsExt},
};

pub const HOME: &str = "/private/var/db/io.galaxybridge/worker";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    pub uid: libc::uid_t,
    pub gid: libc::gid_t,
    user_guid: String,
    group_guid: String,
}

impl Account {
    pub fn parse(text: &str) -> Result<Self> {
        let fields = Record::parse(text, '=')?;
        if fields.len() != 5 || fields.get("version") != Some(&"1") {
            return Err("invalid worker identity receipt version or fields".into());
        }
        let uid = fields.get("uid").ok_or("missing worker UID")?.parse()?;
        let gid = fields.get("gid").ok_or("missing worker GID")?.parse()?;
        if !(60000..=64999).contains(&uid) || !(60000..=64999).contains(&gid) || uid != gid {
            return Err("worker identity is outside its dedicated account range".into());
        }
        let user_guid = fields.get("user_guid").ok_or("missing worker user GUID")?;
        let group_guid = fields
            .get("group_guid")
            .ok_or("missing worker group GUID")?;
        for guid in [user_guid, group_guid] {
            if guid.len() != 36
                || !guid.bytes().enumerate().all(|(index, byte)| {
                    if [8, 13, 18, 23].contains(&index) {
                        byte == b'-'
                    } else {
                        byte.is_ascii_digit() || (b'A'..=b'F').contains(&byte)
                    }
                })
            {
                return Err("invalid worker identity GUID".into());
            }
        }
        if user_guid == group_guid {
            return Err("worker user and group GUIDs must differ".into());
        }
        Ok(Self {
            uid,
            gid,
            user_guid: (*user_guid).into(),
            group_guid: (*group_guid).into(),
        })
    }

    pub fn validate(&self, user: &str, group: &str) -> Result<()> {
        let user = Record::parse(user, ':')?;
        let group = Record::parse(group, ':')?;
        let uid = self.uid.to_string();
        let gid = self.gid.to_string();
        for (field, expected) in [
            ("UniqueID", uid.as_str()),
            ("PrimaryGroupID", gid.as_str()),
            ("GeneratedUID", self.user_guid.as_str()),
            ("AuthenticationAuthority", ";DisabledUser;"),
            ("UserShell", "/usr/bin/false"),
            ("NFSHomeDirectory", HOME),
            ("IsHidden", "1"),
            ("Password", "*"),
        ] {
            if user.get(field) != Some(&expected) {
                return Err(format!(
                    "worker account attribute does not match its receipt: {field}"
                )
                .into());
            }
        }
        for (field, expected) in [
            ("PrimaryGroupID", gid.as_str()),
            ("GeneratedUID", self.group_guid.as_str()),
            ("GroupMembership", "_galaxybridge"),
            ("GroupMembers", self.user_guid.as_str()),
            ("Password", "*"),
        ] {
            if group.get(field) != Some(&expected) {
                return Err(
                    format!("worker group attribute does not match its receipt: {field}").into(),
                );
            }
        }
        Ok(())
    }

    pub fn load() -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
            .open("/Library/PrivilegedHelperTools/io.galaxybridge/IDENTITY")
            .map_err(|_| "install GalaxyBridge to create its dedicated worker identity")?;
        PrivateFile::check(&file, 0)?;
        let mut text = String::new();
        file.take(1025).read_to_string(&mut text)?;
        if text.len() > 1024 {
            return Err("worker identity receipt is too large".into());
        }
        let account = Self::parse(&text)?;
        let user = command::text(
            "/usr/bin/dscl",
            &[
                "/Local/Default",
                "-read",
                "/Users/_galaxybridge",
                "UniqueID",
                "PrimaryGroupID",
                "GeneratedUID",
                "AuthenticationAuthority",
                "UserShell",
                "NFSHomeDirectory",
                "IsHidden",
                "Password",
            ],
        )?;
        let group = command::text(
            "/usr/bin/dscl",
            &[
                "/Local/Default",
                "-read",
                "/Groups/_galaxybridge",
                "PrimaryGroupID",
                "GeneratedUID",
                "GroupMembership",
                "GroupMembers",
                "Password",
            ],
        )?;
        account.validate(&user, &group)?;
        account.lookup()?;
        Ok(account)
    }

    fn lookup(&self) -> Result<()> {
        // All directory lookups and allocation occur in the supervisor, never
        // in its post-fork credential-drop callback.
        let mut storage = vec![0u8; 65536];
        let mut user: libc::passwd = unsafe { std::mem::zeroed() };
        let mut found = std::ptr::null_mut();
        let result = unsafe {
            libc::getpwnam_r(
                c"_galaxybridge".as_ptr(),
                &mut user,
                storage.as_mut_ptr().cast(),
                storage.len(),
                &mut found,
            )
        };
        let matches = |value: *const libc::c_char, expected: &[u8]| {
            !value.is_null() && unsafe { CStr::from_ptr(value) }.to_bytes() == expected
        };
        if result != 0
            || found.is_null()
            || user.pw_uid != self.uid
            || user.pw_gid != self.gid
            || !matches(user.pw_name, b"_galaxybridge")
            || !matches(user.pw_dir, HOME.as_bytes())
            || !matches(user.pw_shell, b"/usr/bin/false")
        {
            return Err("system worker user lookup does not match the local receipt".into());
        }
        let result = unsafe {
            libc::getpwuid_r(
                self.uid,
                &mut user,
                storage.as_mut_ptr().cast(),
                storage.len(),
                &mut found,
            )
        };
        if result != 0
            || found.is_null()
            || user.pw_uid != self.uid
            || user.pw_gid != self.gid
            || !matches(user.pw_name, b"_galaxybridge")
        {
            return Err("numeric worker UID lookup points to a different account".into());
        }
        let mut group: libc::group = unsafe { std::mem::zeroed() };
        let mut found = std::ptr::null_mut();
        let result = unsafe {
            libc::getgrnam_r(
                c"_galaxybridge".as_ptr(),
                &mut group,
                storage.as_mut_ptr().cast(),
                storage.len(),
                &mut found,
            )
        };
        if result != 0
            || found.is_null()
            || group.gr_gid != self.gid
            || !matches(group.gr_name, b"_galaxybridge")
        {
            return Err("system worker group lookup does not match the local receipt".into());
        }
        let result = unsafe {
            libc::getgrgid_r(
                self.gid,
                &mut group,
                storage.as_mut_ptr().cast(),
                storage.len(),
                &mut found,
            )
        };
        if result != 0
            || found.is_null()
            || group.gr_gid != self.gid
            || !matches(group.gr_name, b"_galaxybridge")
        {
            return Err("numeric worker GID lookup points to a different group".into());
        }
        Ok(())
    }
}

pub struct Caller;
impl Caller {
    pub fn home() -> Result<OsString> {
        let mut storage = vec![0u8; 65536];
        let mut user: libc::passwd = unsafe { std::mem::zeroed() };
        let mut found = std::ptr::null_mut();
        let uid = unsafe { libc::geteuid() };
        let result = unsafe {
            libc::getpwuid_r(
                uid,
                &mut user,
                storage.as_mut_ptr().cast(),
                storage.len(),
                &mut found,
            )
        };
        if result != 0 || found.is_null() || user.pw_uid != uid || user.pw_dir.is_null() {
            return Err("cannot determine the inspection user's home directory".into());
        }
        let bytes = unsafe { CStr::from_ptr(user.pw_dir) }.to_bytes();
        if !bytes.starts_with(b"/") {
            return Err("inspection home must be an absolute path".into());
        }
        Ok(OsString::from_vec(bytes.to_vec()))
    }
}

struct Record;
impl Record {
    fn parse(text: &str, separator: char) -> Result<BTreeMap<&str, &str>> {
        let mut fields = BTreeMap::new();
        for line in text.lines() {
            let line = if separator == ':' && line.starts_with("dsAttrTypeNative:IsHidden:") {
                &line["dsAttrTypeNative:".len()..]
            } else {
                line
            };
            let (key, value) = line
                .split_once(separator)
                .ok_or("malformed worker identity record")?;
            let key = key.trim();
            let value = value.trim();
            if key.is_empty() || value.is_empty() || fields.insert(key, value).is_some() {
                return Err("empty or duplicate worker identity attribute".into());
            }
        }
        Ok(fields)
    }
}
