pub mod cli;
pub mod ipc;
#[cfg(target_os = "macos")]
pub mod macos;
pub mod rndis;
#[cfg(target_os = "macos")]
pub mod service;
pub mod usb;
pub mod worker;

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T> = std::result::Result<T, Error>;
