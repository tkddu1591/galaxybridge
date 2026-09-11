//! Fail-closed selection of one RNDIS function in a USB configuration.
use super::devices;
use crate::Result;
use nusb::descriptors::{ConfigurationDescriptor, InterfaceDescriptor, TransferType};
use std::collections::BTreeSet;

pub const CONFIGURATION_LIMIT: usize = u16::MAX as usize;

#[derive(Debug)]
pub struct Interface {
    pub number: u8,
    pub alternate: u8,
}

#[derive(Debug)]
pub struct Layout {
    pub control: Interface,
    pub data: Interface,
    pub notification: u8,
    pub incoming: u8,
    pub outgoing: u8,
}

impl Layout {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > CONFIGURATION_LIMIT {
            return Err("USB configuration exceeds 65535 bytes".into());
        }
        let config = ConfigurationDescriptor::new(bytes).ok_or("invalid USB configuration")?;
        if config.as_bytes().len() != bytes.len() {
            return Err("data after the declared USB configuration".into());
        }
        // nusb's descriptor iterators stop at malformed records. Validate the
        // complete chain first so a malformed tail cannot hide a second function.
        let mut rest = bytes;
        let mut associations = Vec::new();
        while !rest.is_empty() {
            if rest.len() < 2 || rest[0] < 2 || rest[0] as usize > rest.len() {
                return Err("truncated USB descriptor chain".into());
            }
            let descriptor = &rest[..rest[0] as usize];
            let minimum = match descriptor[1] {
                2 | 4 => 9,
                5 => 7,
                11 => 8,
                0x24 => 3,
                _ => 2,
            };
            if descriptor.len() < minimum || (descriptor[1] == 2 && rest.len() != bytes.len()) {
                return Err("invalid USB descriptor length or nested configuration".into());
            }
            if descriptor[1] == 11 {
                let first = descriptor[2] as u16;
                let end = first + descriptor[3] as u16;
                if first == end || end > 256 {
                    return Err("invalid USB interface association range".into());
                }
                if associations
                    .iter()
                    .any(|&(start, stop)| first < stop && start < end)
                {
                    return Err("overlapping USB interface associations".into());
                }
                associations.push((first, end));
            }
            rest = &rest[descriptor.len()..];
        }

        let interfaces: Vec<_> = config.interface_alt_settings().collect();
        let mut alternates = BTreeSet::new();
        let mut numbers = BTreeSet::new();
        for interface in &interfaces {
            if !alternates.insert((interface.interface_number(), interface.alternate_setting())) {
                return Err("duplicate USB interface alternate setting".into());
            }
            numbers.insert(interface.interface_number());
            let mut addresses = BTreeSet::new();
            for endpoint in interface.endpoints() {
                if endpoint.address() & 0x70 != 0
                    || endpoint.address() & 0x0f == 0
                    || !addresses.insert(endpoint.address())
                {
                    return Err("invalid or duplicate USB endpoint address".into());
                }
            }
            if addresses.len() != interface.num_endpoints() as usize {
                return Err("USB endpoint count disagrees with descriptors".into());
            }
        }
        if numbers.len() != config.num_interfaces() as usize {
            return Err("USB interface count disagrees with descriptors".into());
        }
        let controls: Vec<_> = interfaces
            .iter()
            .filter(|i| devices::signature(i.class(), i.subclass(), i.protocol()))
            .collect();
        if controls.len() != 1 {
            return Err("missing or ambiguous RNDIS control interface".into());
        }
        let control = controls[0];
        let mut union = None;
        for descriptor in control.descriptors() {
            if descriptor[1] == 0x24
                && descriptor[2] == 6
                && (descriptor.len() != 5
                    || descriptor[3] != control.interface_number()
                    || descriptor[4] == control.interface_number()
                    || union.replace(descriptor[4]).is_some())
            {
                return Err("malformed or ambiguous CDC Union descriptor".into());
            }
        }
        // A number of RNDIS devices omit the Union descriptor. Only in its
        // absence do we support the conventional adjacent data interface.
        let data_number = union
            .or_else(|| control.interface_number().checked_add(1))
            .ok_or("missing RNDIS data interface")?;
        let association = associations
            .iter()
            .find(|&&(start, end)| (start..end).contains(&(control.interface_number() as u16)));
        if let Some(&(start, end)) = association {
            if start != control.interface_number() as u16
                || !(start..end).contains(&(data_number as u16))
            {
                return Err("RNDIS data interface is outside its USB function".into());
            }
        } else if associations
            .iter()
            .any(|&(start, end)| (start..end).contains(&(data_number as u16)))
        {
            return Err("RNDIS data interface belongs to another USB function".into());
        }
        let candidates: Vec<_> = interfaces
            .iter()
            .filter(|i| i.interface_number() == data_number && i.class() == 0x0a)
            .filter(|i| i.num_endpoints() != 0)
            .collect();
        if candidates.len() != 1 {
            return Err("missing or ambiguous RNDIS data alternate setting".into());
        }
        let data = candidates[0];
        let notification = Endpoints::notification(control)?;
        let (incoming, outgoing) = Endpoints::data(data)?;
        if notification == incoming {
            return Err("RNDIS interfaces share an endpoint address".into());
        }
        Ok(Self {
            control: Interface {
                number: control.interface_number(),
                alternate: control.alternate_setting(),
            },
            data: Interface {
                number: data.interface_number(),
                alternate: data.alternate_setting(),
            },
            notification,
            incoming,
            outgoing,
        })
    }
}

struct Endpoints;
impl Endpoints {
    fn notification(interface: &InterfaceDescriptor<'_>) -> Result<u8> {
        let endpoints: Vec<_> = interface.endpoints().collect();
        if endpoints.len() != 1 {
            return Err("ambiguous RNDIS notification endpoint".into());
        }
        let endpoint = &endpoints[0];
        let raw = endpoint.as_bytes();
        if endpoint.transfer_type() != TransferType::Interrupt
            || endpoint.address() & 0x80 == 0
            || !(8..=1024).contains(&endpoint.max_packet_size())
            || raw[5] & 0xe0 != 0
            || raw[6] == 0
        {
            return Err("invalid RNDIS notification endpoint".into());
        }
        Ok(endpoint.address())
    }

    fn data(interface: &InterfaceDescriptor<'_>) -> Result<(u8, u8)> {
        let endpoints: Vec<_> = interface.endpoints().collect();
        if endpoints.len() != 2 {
            return Err("RNDIS needs exactly two data endpoints".into());
        }
        let mut incoming = None;
        let mut outgoing = None;
        for endpoint in endpoints {
            if endpoint.transfer_type() != TransferType::Bulk
                || !matches!(endpoint.max_packet_size(), 8 | 16 | 32 | 64 | 512 | 1024)
                || endpoint.as_bytes()[5] & 0xf8 != 0
            {
                return Err("invalid RNDIS bulk endpoint".into());
            }
            let slot = if endpoint.address() & 0x80 != 0 {
                &mut incoming
            } else {
                &mut outgoing
            };
            if slot.replace(endpoint.address()).is_some() {
                return Err("duplicate RNDIS data endpoint direction".into());
            }
        }
        Ok((
            incoming.ok_or("missing RNDIS bulk IN")?,
            outgoing.ok_or("missing RNDIS bulk OUT")?,
        ))
    }
}
