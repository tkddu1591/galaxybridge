use crate::Result;
use nusb::{DeviceInfo, MaybeFuture};

#[derive(Clone, Debug, Default)]
pub struct Filter {
    pub product: Option<u16>,
    pub serial: Option<String>,
}
impl Filter {
    pub fn matches(&self, device: &DeviceInfo) -> bool {
        device.vendor_id() == 0x04e8
            && self.product.is_none_or(|p| p == device.product_id())
            && self
                .serial
                .as_deref()
                .is_none_or(|s| device.serial_number() == Some(s))
            && device
                .interfaces()
                .any(|i| signature(i.class(), i.subclass(), i.protocol()))
    }
    pub fn list(&self) -> Result<Vec<DeviceInfo>> {
        Ok(nusb::list_devices()
            .wait()?
            .filter(|d| self.matches(d))
            .collect())
    }
    pub fn select(&self) -> Result<DeviceInfo> {
        let mut devices = self.list()?;
        if devices.len() != 1 {
            return Err(format!(
                "expected one matching Samsung RNDIS device; found {}",
                devices.len()
            )
            .into());
        }
        Ok(devices.remove(0))
    }
}

pub fn signature(class: u8, subclass: u8, protocol: u8) -> bool {
    matches!(
        (class, subclass, protocol),
        (0xe0, 1, 3) | (0xef, 4, 1) | (2, 2, 0xff)
    )
}
