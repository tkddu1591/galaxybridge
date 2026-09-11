use super::{devices::Filter, layout::Layout};
use crate::{
    Result,
    rndis::{
        control::{self, Request},
        wire,
    },
};
use nusb::{
    Device, Endpoint, Interface, MaybeFuture,
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
        let layout = Layout::parse(config.as_bytes())?;
        let control = device
            .claim_interface(layout.control.number)
            .wait()
            .map_err(|e| format!("claim control interface {}: {e}", layout.control.number))?;
        control.set_alt_setting(layout.control.alternate).wait()?;
        let data_intf = device
            .claim_interface(layout.data.number)
            .wait()
            .map_err(|e| format!("claim data interface {}: {e}", layout.data.number))?;
        data_intf.set_alt_setting(layout.data.alternate).wait()?;
        let rx = data_intf.endpoint::<Bulk, In>(layout.incoming)?;
        let tx = data_intf.endpoint::<Bulk, Out>(layout.outgoing)?;
        let mut notifications = control.endpoint::<Interrupt, In>(layout.notification)?;
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
        let mut responses = 0;
        while Instant::now() < deadline {
            let Some(completion) = self
                .notifications
                .wait_next_complete(Duration::from_millis(100))
            else {
                continue;
            };
            let mut notification = completion.into_result()?;
            control::Notification::validate(&notification)?;
            responses += 1;
            if responses > 32 {
                return Err("too many RNDIS control indications without a completion".into());
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
            if bytes == [0] {
                std::thread::sleep(Duration::from_millis(10));
                continue;
            }
            // Media status indications can precede the command completion.
            if wire::u32_at(&bytes, 0)? == 7 {
                if control::Indication::parse(&bytes)? == control::Indication::MediaDisconnect {
                    return Err("RNDIS media disconnected".into());
                }
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
