//! Defense in depth against invoking an unsigned development USB helper.
use crate::Result;

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn gb_usb_entitlements_check() -> libc::c_int;
}

pub struct Confinement;
impl Confinement {
    pub fn check() -> Result<()> {
        #[cfg(target_os = "macos")]
        // SAFETY: the native function inspects only this process's code-signing
        // entitlements, retains/releases its CF values, and takes no pointers.
        if unsafe { gb_usb_entitlements_check() } == 1 {
            return Ok(());
        }
        Err("USB access requires the installed App Sandbox worker with USB entitlement".into())
    }
}
