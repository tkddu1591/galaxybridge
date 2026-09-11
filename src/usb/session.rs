use super::devices::{self, Filter};
use crate::{
    Result,
    rndis::{
        control::{self, Request},
        wire,
    },
};
use nusb::{
    Device, Endpoint, Interface, MaybeFuture,
    descriptors::TransferType,
    transfer::{Buffer, Bulk, ControlIn, ControlOut, ControlType, In, Interrupt, Out, Recipient},
};
use std::time::{Duration, Instant};

pub struct Session {
    _device: Device,
    control: Interface,
    notifications: Endpoint<Interrupt, In>,
    _data: Interface,
    id: u32,
    pub incoming: Option<Endpoint<Bulk, In>>,
    pub outgoing: Option<Endpoint<Bulk, Out>>,
    pub mac: [u8; 6],
    pub max_transfer: usize,
}

impl Session {
    pub fn open(filter: &Filter) -> Result<Self> {
        let device = filter.select()?.open().wait()?;
        let config = device.active_configuration()?;
        let controls: Vec<_> = config
            .interface_alt_settings()
            .filter(|i| devices::signature(i.class(), i.subclass(), i.protocol()))
            .collect();
        if controls.len() != 1 {
            return Err("ambiguous RNDIS control interface".into());
        }
        let ctl = &controls[0];
        let data_number = ctl
            .descriptors()
            .find_map(|d| {
                if d.len() == 5 && d[1] == 0x24 && d[2] == 6 && d[3] == ctl.interface_number() {
                    Some(d[4])
                } else {
                    None
                }
            })
            .or_else(|| ctl.interface_number().checked_add(1))
            .ok_or("missing data interface")?;
        let data: Vec<_> = config
            .interface_alt_settings()
            .filter(|i| i.interface_number() == data_number && i.class() == 0x0a)
            .filter(|i| {
                i.endpoints()
                    .filter(|e| e.transfer_type() == TransferType::Bulk)
                    .count()
                    == 2
            })
            .collect();
        if data.len() != 1 {
            return Err("missing or ambiguous RNDIS data alternate setting".into());
        }
        let data_desc = &data[0];
        let incoming = data_desc
            .endpoints()
            .find(|e| e.transfer_type() == TransferType::Bulk && e.address() & 0x80 != 0)
            .ok_or("missing bulk IN")?
            .address();
        let outgoing = data_desc
            .endpoints()
            .find(|e| e.transfer_type() == TransferType::Bulk && e.address() & 0x80 == 0)
            .ok_or("missing bulk OUT")?
            .address();
        let notification = ctl
            .endpoints()
            .find(|e| e.transfer_type() == TransferType::Interrupt && e.address() & 0x80 != 0)
            .ok_or("missing RNDIS notification endpoint")?
            .address();
        let control = device
            .claim_interface(ctl.interface_number())
            .wait()
            .map_err(|e| format!("claim control interface {}: {e}", ctl.interface_number()))?;
        if ctl.alternate_setting() != 0 {
            control.set_alt_setting(ctl.alternate_setting()).wait()?;
        }
        let data_intf = device
            .claim_interface(data_number)
            .wait()
            .map_err(|e| format!("claim data interface {data_number}: {e}"))?;
        if data_desc.alternate_setting() != 0 {
            data_intf
                .set_alt_setting(data_desc.alternate_setting())
                .wait()?;
        }
        let rx = data_intf.endpoint::<Bulk, In>(incoming)?;
        let tx = data_intf.endpoint::<Bulk, Out>(outgoing)?;
        let mut notifications = control.endpoint::<Interrupt, In>(notification)?;
        notifications.submit(Buffer::new(notifications.max_packet_size()));
        let mut session = Self {
            _device: device,
            control,
            notifications,
            _data: data_intf,
            id: 1,
            incoming: Some(rx),
            outgoing: Some(tx),
            mac: [0; 6],
            max_transfer: 0,
        };
        let init = session.exchange(Request::initialize(session.id))?;
        session.max_transfer = control::Negotiation::parse(&init)?.max_transfer;
        let id = session.next_id();
        let response = session.exchange(Request::query(id, control::MAC_ADDRESS))?;
        session.mac = control::Query::data(&response)?
            .try_into()
            .map_err(|_| "invalid MAC response length")?;
        if session.mac == [0; 6] || session.mac[0] & 1 != 0 {
            return Err("invalid device MAC".into());
        }
        let id = session.next_id();
        session.exchange(Request::filter(id))?;
        Ok(session)
    }

    fn next_id(&mut self) -> u32 {
        self.id = self.id.wrapping_add(1).max(1);
        self.id
    }

    fn send(&self, request: &Request) -> Result<()> {
        self.control
            .control_out(
                ControlOut {
                    control_type: ControlType::Class,
                    recipient: Recipient::Interface,
                    request: 0,
                    value: 0,
                    index: self.control.interface_number() as u16,
                    data: &request.bytes,
                },
                Duration::from_millis(500),
            )
            .wait()?;
        Ok(())
    }

    fn exchange(&mut self, request: Request) -> Result<Vec<u8>> {
        self.send(&request)?;
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            let Some(completion) = self
                .notifications
                .wait_next_complete(Duration::from_millis(100))
            else {
                continue;
            };
            let mut notification = completion.into_result()?;
            if notification.len() != 8
                || wire::u32_at(&notification, 0)? != 1
                || wire::u32_at(&notification, 4)? != 0
            {
                return Err("invalid RESPONSE_AVAILABLE notification".into());
            }
            notification.clear();
            self.notifications.submit(notification);
            let bytes = self
                .control
                .control_in(
                    ControlIn {
                        control_type: ControlType::Class,
                        recipient: Recipient::Interface,
                        request: 1,
                        value: 0,
                        index: self.control.interface_number() as u16,
                        length: control::TRANSFER_LIMIT as u16,
                    },
                    Duration::from_millis(500),
                )
                .wait()?;
            if bytes.len() < 8 {
                std::thread::sleep(Duration::from_millis(10));
                continue;
            }
            // Media status indications can precede the command completion.
            if wire::u32_at(&bytes, 0)? == 7 {
                continue;
            }
            return Ok(request.response(&bytes)?.to_vec());
        }
        Err("RNDIS control response timed out".into())
    }

    pub fn keepalive(&mut self) -> Result<()> {
        let id = self.next_id();
        self.exchange(Request::keepalive(id))?;
        Ok(())
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.send(&Request::halt(self.id.wrapping_add(1)));
    }
}
