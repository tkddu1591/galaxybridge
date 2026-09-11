use crate::Result;
use nusb::{DeviceInfo, MaybeFuture};

#[derive(Clone, Debug, Default)]
pub struct Filter {
    pub vendor: Option<u16>,
    pub product: Option<u16>,
    pub serial: Option<String>,
}

pub struct Identity<'a> {
    pub vendor: u16,
    pub product: u16,
    pub serial: Option<&'a str>,
}

impl Filter {
    pub fn includes(&self, identity: &Identity<'_>) -> bool {
        self.vendor.is_none_or(|vendor| vendor == identity.vendor)
            && self
                .product
                .is_none_or(|product| product == identity.product)
            && self
                .serial
                .as_deref()
                .is_none_or(|serial| identity.serial == Some(serial))
    }

    pub fn matches(&self, device: &DeviceInfo) -> bool {
        self.includes(&Identity {
            vendor: device.vendor_id(),
            product: device.product_id(),
            serial: device.serial_number(),
        }) && device
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
                "expected one matching RNDIS device; found {}",
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
