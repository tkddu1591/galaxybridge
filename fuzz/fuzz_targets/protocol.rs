#![no_main]
use galaxybridge::{
    ipc::Message,
    rndis::{
        control::{Indication, Negotiation, Notification, Query, Request, TRANSFER_LIMIT},
        packet,
    },
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    if let Ok(frames) = packet::decode(bytes) {
        assert!(bytes.len() <= TRANSFER_LIMIT);
        assert!(frames.len() <= TRANSFER_LIMIT / (44 + 14));
        for frame in frames {
            assert!((14..=1514).contains(&frame.len()));
            assert!(frame.as_ptr() >= bytes.as_ptr());
            assert!(frame.as_ptr_range().end <= bytes.as_ptr_range().end);
            // Accepted payloads must round-trip without byte changes for each
            // supported USB bulk packet size, including terminator boundaries.
            for size in [64, 512, 1024] {
                let encoded = packet::encode(frame, size, TRANSFER_LIMIT).unwrap();
                assert_eq!(packet::decode(&encoded).unwrap(), [frame]);
            }
        }
    }
    if let Ok(data) = Query::data(bytes) {
        if !data.is_empty() {
            assert!(data.as_ptr() >= bytes.as_ptr());
            assert!(data.as_ptr_range().end <= bytes.as_ptr_range().end);
        }
        assert!(bytes.len() <= TRANSFER_LIMIT);
    }
    if let Ok(negotiation) = Negotiation::parse(bytes) {
        assert!((1558..=TRANSFER_LIMIT).contains(&negotiation.max_transfer));
    }
    if Notification::validate(bytes).is_ok() {
        assert_eq!(bytes, [1, 0, 0, 0, 0, 0, 0, 0]);
    }
    let _ = Indication::parse(bytes);
    for request in [
        Request::initialize(1),
        Request::query(1, 0),
        Request::filter(1),
        Request::keepalive(1),
    ] {
        if let Ok(response) = request.response(bytes) {
            assert!(response.len() <= bytes.len());
            assert_eq!(u32::from_le_bytes(response[8..12].try_into().unwrap()), 1);
            assert_eq!(u32::from_le_bytes(response[12..16].try_into().unwrap()), 0);
        }
    }
    if let Ok(message) = Message::decode(bytes) {
        assert_eq!(message.encode(), bytes);
    }
});
