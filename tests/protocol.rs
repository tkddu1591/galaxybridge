use galaxybridge::{
    ipc::Message,
    rndis::{
        control::{Negotiation, Query, Request, TRANSFER_LIMIT},
        packet,
    },
    usb::devices,
};

fn words(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}
fn frame() -> Vec<u8> {
    (0..100).map(|i| i as u8).collect()
}

#[test]
fn ethernet_roundtrip_and_transfer_termination() {
    for len in 14..=1514 {
        let frame = vec![42; len];
        let encoded = packet::encode(&frame, 512, TRANSFER_LIMIT).unwrap();
        assert_ne!(encoded.len() % 512, 0);
        assert_eq!(packet::decode(&encoded).unwrap(), vec![frame.as_slice()]);
    }
}

#[test]
fn usb_terminator_is_not_part_of_rndis_message_length() {
    // Microsoft USB Short Packets: the final zero belongs to the USB transfer,
    // not MessageLength. Cover full-, high-, and super-speed packet sizes.
    for usb_packet_size in [64, 512, 1024] {
        let frame = vec![0xa5; usb_packet_size - 44];
        let encoded = packet::encode(&frame, usb_packet_size, TRANSFER_LIMIT).unwrap();
        let message_length = u32::from_le_bytes(encoded[4..8].try_into().unwrap()) as usize;
        assert_eq!(message_length, usb_packet_size);
        assert_eq!(encoded.len(), usb_packet_size + 1);
        assert_eq!(encoded.last(), Some(&0));
        assert_eq!(packet::decode(&encoded).unwrap(), [frame.as_slice()]);
        // The device transfer bound must still account for the extra USB byte.
        assert!(packet::encode(&frame, usb_packet_size, usb_packet_size).is_err());
        assert!(packet::encode(&frame, usb_packet_size, usb_packet_size + 1).is_ok());
    }
}

#[test]
fn usb_padding_larger_than_rndis_header_is_accepted_but_not_nonzero_trailers() {
    let frame = vec![0xa5; 60];
    let message = [words(&[1, 104, 36, 60, 0, 0, 0, 0, 0, 0, 0]), frame.clone()].concat();
    for transfer_length in [148, 512, 1024, TRANSFER_LIMIT] {
        let mut bytes = message.clone();
        bytes.resize(transfer_length, 0);
        assert_eq!(packet::decode(&bytes).unwrap(), [frame.as_slice()]);
        // A valid prefix must not hide garbage, including at the very end.
        bytes[transfer_length - 1] = 0xff;
        assert!(packet::decode(&bytes).is_err());
    }
    let mut oversized = message;
    oversized.resize(TRANSFER_LIMIT + 1, 0);
    assert!(packet::decode(&oversized).is_err());
}

#[test]
fn aggregate_internal_padding_is_distinct_from_usb_trailing_padding() {
    // First message aligns the second to eight bytes; those seven padding
    // bytes are included in MessageLength and need not contain zeroes.
    let first = [
        words(&[1, 112, 36, 61, 0, 0, 0, 0, 0, 0, 0]),
        vec![0x42; 61],
        vec![0xa5; 7],
    ]
    .concat();
    let second = [
        words(&[1, 104, 36, 60, 0, 0, 0, 0, 0, 0, 0]),
        vec![0x24; 60],
    ]
    .concat();
    let mut bytes = [first, second].concat();
    bytes.resize(512, 0);
    let frames = packet::decode(&bytes).unwrap();
    assert_eq!(frames, [vec![0x42; 61], vec![0x24; 60]]);
}

#[test]
fn packet_payload_cannot_escape_into_the_next_message() {
    let first = [
        words(&[1, 58, 36, 100, 0, 0, 0, 0, 0, 0, 0]),
        vec![0x42; 14],
    ]
    .concat();
    let second = [
        words(&[1, 144, 36, 100, 0, 0, 0, 0, 0, 0, 0]),
        vec![0x24; 100],
    ]
    .concat();
    // The total transfer contains enough bytes, but the first message does not.
    assert!(packet::decode(&[first, second].concat()).is_err());
}

#[test]
fn query_payload_cannot_escape_declared_control_message_length() {
    let bytes = [
        words(&[0x8000_0004, 24, 9, 0, 6, 16]),
        vec![2, 1, 2, 3, 4, 5],
    ]
    .concat();
    let request = Request::query(9, 0x0101_0102);
    let declared_message = request.response(&bytes).unwrap();
    assert_eq!(declared_message.len(), 24);
    assert!(Query::data(declared_message).is_err());
}

#[test]
fn every_unsupported_packet_metadata_field_is_rejected() {
    let message = [
        words(&[1, 104, 36, 60, 0, 0, 0, 0, 0, 0, 0]),
        vec![0x42; 60],
    ]
    .concat();
    for offset in [16, 20, 24, 28, 32, 36, 40] {
        for value in [1u32, u32::MAX] {
            let mut bytes = message.clone();
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            assert!(packet::decode(&bytes).is_err(), "metadata offset {offset}");
        }
    }
}

#[test]
fn aggregated_packets_follow_message_length_including_padding() {
    let a = packet::encode(&frame(), 512, TRANSFER_LIMIT).unwrap();
    let b = packet::encode(&[7; 60], 512, TRANSFER_LIMIT).unwrap();
    let bytes = [a, b].concat();
    let frames = packet::decode(&bytes).unwrap();
    assert_eq!(frames.len(), 2);
    assert_eq!(frames[0], frame());
    assert_eq!(frames[1], [7; 60]);
}

#[test]
fn packed_android_aggregates_preserve_frames_at_every_byte_alignment() {
    for remainder in 0..8 {
        let first = vec![0x42; 60 + remainder];
        let second = vec![0x24; 61];
        let third = vec![0x81; 63];
        // These lengths never need a USB terminator. Each message immediately
        // follows its predecessor's MessageLength, including odd offsets.
        let mut transfer = [first.as_slice(), second.as_slice(), third.as_slice()]
            .into_iter()
            .flat_map(|frame| packet::encode(frame, 512, TRANSFER_LIMIT).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            packet::decode(&transfer).unwrap(),
            [first.as_slice(), second.as_slice(), third.as_slice()]
        );
        transfer.resize(512, 0);
        assert_eq!(
            packet::decode(&transfer).unwrap(),
            [first.as_slice(), second.as_slice(), third.as_slice()]
        );
    }
}

#[test]
fn malformed_second_message_in_packed_aggregate_fails_the_entire_transfer() {
    let first = packet::encode(&[0x42; 61], 512, TRANSFER_LIMIT).unwrap();
    let second = packet::encode(&[0x24; 60], 512, TRANSFER_LIMIT).unwrap();
    for (offset, value) in [
        (0, 7u32),
        (4, 0),
        (4, 43),
        (4, u32::MAX),
        (8, 0),
        (8, u32::MAX),
        (12, 61),
        (12, u32::MAX),
        (20, 1),
        (32, 1),
    ] {
        let mut malformed = second.clone();
        malformed[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        let transfer = [first.clone(), malformed].concat();
        assert!(
            packet::decode(&transfer).is_err(),
            "second header field {offset}"
        );
    }
    for length in 1..44 {
        let mut transfer = [first.clone(), second.clone()].concat();
        transfer.extend(vec![0x7f; length]);
        assert!(packet::decode(&transfer).is_err());
    }
    // A valid later header never permits skipping corruption between messages.
    let transfer = [first, vec![0; 7], second].concat();
    assert!(packet::decode(&transfer).is_err());
}

#[test]
fn malformed_offsets_lengths_and_metadata_are_rejected() {
    let valid = packet::encode(&frame(), 512, TRANSFER_LIMIT).unwrap();
    for (offset, value) in [
        (4, 0),
        (4, 43),
        (4, u32::MAX),
        (8, 0),
        (8, u32::MAX),
        (12, 0),
        (12, 1515),
        (12, u32::MAX),
        (20, 1),
        (32, 1),
    ] {
        let mut bad = valid.clone();
        bad[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        assert!(
            packet::decode(&bad).is_err(),
            "offset {offset}, value {value}"
        );
    }
    for n in 1..valid.len() {
        assert!(packet::decode(&valid[..n]).is_err());
    }
}

#[test]
fn oversized_transfers_and_outbound_frames_are_rejected() {
    assert!(packet::decode(&vec![0; TRANSFER_LIMIT + 1]).is_err());
    assert!(packet::encode(&[0; 1515], 512, TRANSFER_LIMIT).is_err());
    assert!(packet::encode(&frame(), 0, TRANSFER_LIMIT).is_err());
    assert!(packet::encode(&frame(), 512, 50).is_err());
}

#[test]
fn control_completions_require_matching_request_and_success() {
    let request = Request::keepalive(77);
    let valid = words(&[0x8000_0008, 16, 77, 0]);
    assert!(request.response(&valid).is_ok());
    for (offset, value) in [(0, 8u32), (4, 8), (4, 100), (8, 76), (12, 1)] {
        let mut bad = valid.clone();
        bad[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        assert!(request.response(&bad).is_err());
    }
}

#[test]
fn initialization_limits_and_query_bounds_are_checked() {
    let valid = words(&[0x8000_0002, 52, 1, 0, 1, 0, 1, 0, 15, 23700, 4, 0, 0]);
    assert_eq!(
        Negotiation::parse(&valid).unwrap().max_transfer,
        TRANSFER_LIMIT
    );
    for (offset, value) in [(16, 2u32), (24, 0), (28, 1), (32, 0), (36, 100), (40, 8)] {
        let mut bad = valid.clone();
        bad[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        assert!(Negotiation::parse(&bad).is_err());
    }
    let query = [
        words(&[0x8000_0004, 30, 9, 0, 6, 16]),
        vec![2, 1, 2, 3, 4, 5],
    ]
    .concat();
    assert_eq!(Query::data(&query).unwrap(), [2, 1, 2, 3, 4, 5]);
    for value in [0u32, 8, 17, u32::MAX] {
        let mut bad = query.clone();
        bad[20..24].copy_from_slice(&value.to_le_bytes());
        assert!(Query::data(&bad).is_err());
    }
}

#[test]
fn root_ipc_accepts_only_unicast_mac_or_bounded_ethernet_frames() {
    assert!(Message::decode(&[1, 2, 3, 4, 5, 6, 7]).is_ok());
    for bytes in [
        vec![],
        vec![1; 7],
        vec![1, 0, 0, 0, 0, 0, 0],
        vec![3; 30],
        vec![2; 14],
        vec![2; 1516],
    ] {
        assert!(Message::decode(&bytes).is_err());
    }
    assert!(matches!(Message::decode(&[2; 1515]), Ok(Message::Frame(_))));
}

#[test]
fn deterministic_hostile_input_corpus_does_not_panic() {
    let mut seed = 0x7abc_e012_8345_0987u64;
    for n in 0..10_000 {
        let mut bytes = vec![0; n % 2048];
        for byte in &mut bytes {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            *byte = seed as u8;
        }
        let _ = packet::decode(&bytes);
        let _ = Query::data(&bytes);
        let _ = Negotiation::parse(&bytes);
        let _ = Request::initialize(1).response(&bytes);
        let _ = Message::decode(&bytes);
    }
}

#[test]
fn rndis_signature_does_not_match_mass_storage_or_cdc_ncm() {
    assert!(devices::signature(0xe0, 1, 3));
    assert!(devices::signature(0xef, 4, 1));
    assert!(devices::signature(2, 2, 0xff));
    assert!(!devices::signature(8, 6, 0x50));
    assert!(!devices::signature(2, 0x0d, 0));
}

#[cfg(target_os = "macos")]
#[test]
fn route_parser_and_interface_names_reject_command_text() {
    use galaxybridge::macos::{interface::Pair, route::Snapshot};
    assert!(Snapshot::parse("gateway: 10.0.0.1\ninterface: feth3").is_some());
    assert!(Snapshot::parse("gateway: $(id)\ninterface: en0").is_none());
    assert!(Snapshot::parse("gateway: 10.0.0.1\ninterface: en0;id").is_none());
    assert!(Pair::valid_name("feth999"));
    for bad in ["en0", "feth", "feth3;id", "feth123456", "feth-1", "feth0\n"] {
        assert!(!Pair::valid_name(bad));
    }
}

#[cfg(target_os = "macos")]
#[test]
fn bpf_framing_rejects_truncation_and_mismatched_capture_lengths() {
    use galaxybridge::macos::bpf::Batch;
    let mut bytes = vec![0; 120];
    bytes[8..12].copy_from_slice(&100u32.to_ne_bytes());
    bytes[12..16].copy_from_slice(&100u32.to_ne_bytes());
    bytes[16..18].copy_from_slice(&20u16.to_ne_bytes());
    assert_eq!(Batch::decode(&bytes, [8, 12, 16, 4]).unwrap()[0].len(), 100);
    assert!(Batch::decode(&bytes[..119], [8, 12, 16, 4]).is_err());
    bytes[12..16].copy_from_slice(&200u32.to_ne_bytes());
    assert!(Batch::decode(&bytes, [8, 12, 16, 4]).is_err());
}
