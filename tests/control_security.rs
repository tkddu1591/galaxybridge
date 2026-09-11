use galaxybridge::rndis::{
    control::{Indication, Negotiation, Notification, Query, Request, TRANSFER_LIMIT},
    packet,
};

struct Fixture;
impl Fixture {
    fn words(values: &[u32]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_le_bytes()).collect()
    }
}

#[test]
fn notifications_require_exact_size_type_and_reserved_bytes() {
    let valid = Fixture::words(&[1, 0]);
    Notification::validate(&valid).unwrap();
    for end in 0..valid.len() {
        assert!(Notification::validate(&valid[..end]).is_err());
    }
    for position in 0..8 {
        for value in 0u8..=255 {
            if value == valid[position] {
                continue;
            }
            let mut bytes = valid.clone();
            bytes[position] = value;
            assert!(Notification::validate(&bytes).is_err());
        }
    }
    assert!(Notification::validate(&[valid, vec![0]].concat()).is_err());
}

#[test]
fn status_indications_validate_their_own_message_and_payload_boundary() {
    let connected = Fixture::words(&[7, 20, 0x4001_000b, 0, 0]);
    let disconnected = Fixture::words(&[7, 20, 0x4001_000c, 0, 0]);
    assert_eq!(
        Indication::parse(&connected).unwrap(),
        Indication::MediaConnect
    );
    assert_eq!(
        Indication::parse(&disconnected).unwrap(),
        Indication::MediaDisconnect
    );
    for (word, value) in [(0, 8u32), (1, 8), (1, 21), (3, 1), (3, u32::MAX), (4, 20)] {
        let mut bytes = connected.clone();
        bytes[word * 4..word * 4 + 4].copy_from_slice(&value.to_le_bytes());
        assert!(Indication::parse(&bytes).is_err());
    }
    let diagnostic = Fixture::words(&[7, 28, 0xc001_0015, 8, 20, 1, 4]);
    assert_eq!(Indication::parse(&diagnostic).unwrap(), Indication::Other);
    for offset in [0u32, 19, 21, u32::MAX] {
        let mut bytes = diagnostic.clone();
        bytes[16..20].copy_from_slice(&offset.to_le_bytes());
        assert!(Indication::parse(&bytes).is_err());
    }
}

#[test]
fn query_and_negotiation_do_not_trust_their_callers_to_check_headers() {
    let query = [Fixture::words(&[0x8000_0004, 30, 1, 0, 6, 16]), vec![2; 6]].concat();
    for (word, value) in [(0, 4u32), (1, 24), (1, 31), (3, 1), (4, u32::MAX)] {
        let mut bytes = query.clone();
        bytes[word * 4..word * 4 + 4].copy_from_slice(&value.to_le_bytes());
        assert!(Query::data(&bytes).is_err());
    }
    assert!(Query::data(&Fixture::words(&[0x8000_0004, 24, 1, 0, 0, 16])).is_err());
    assert!(
        Query::data(&Fixture::words(&[0x8000_0004, 24, 1, 0, 0, 0]))
            .unwrap()
            .is_empty()
    );

    let initialize = Fixture::words(&[0x8000_0002, 52, 1, 0, 1, 0, 1, 0, 1, 16384, 0, 0, 0]);
    for (word, value) in [(0, 2u32), (1, 48), (3, 1), (6, 3), (11, 52), (12, 1)] {
        let mut bytes = initialize.clone();
        bytes[word * 4..word * 4 + 4].copy_from_slice(&value.to_le_bytes());
        assert!(Negotiation::parse(&bytes).is_err());
    }
    for mut bytes in [query, initialize] {
        bytes.resize(TRANSFER_LIMIT + 1, 0);
        assert!(Query::data(&bytes).is_err());
        assert!(Negotiation::parse(&bytes).is_err());
    }
}

#[test]
fn completion_type_requires_its_full_fixed_header() {
    for request in [Request::initialize(1), Request::query(1, 0)] {
        let short = Fixture::words(&[request.kind | 0x8000_0000, 16, 1, 0]);
        assert!(request.response(&short).is_err());
    }
}

#[test]
fn packet_offsets_and_aggregate_starts_obey_rndis_alignment() {
    let mut packet = Fixture::words(&[1, 105, 37, 60, 0, 0, 0, 0, 0, 0, 0]);
    packet.push(0);
    packet.extend([0x42; 60]);
    assert!(packet::decode(&packet).is_err());
    let first = [
        Fixture::words(&[1, 105, 36, 61, 0, 0, 0, 0, 0, 0, 0]),
        vec![0x42; 61],
    ]
    .concat();
    let second = [
        Fixture::words(&[1, 104, 36, 60, 0, 0, 0, 0, 0, 0, 0]),
        vec![0x24; 60],
    ]
    .concat();
    assert!(packet::decode(&[first, second].concat()).is_err());
}

#[test]
fn valid_packet_offsets_and_all_frame_lengths_preserve_payloads() {
    for length in 14..=1514 {
        for gap in [0, 4, 8, 60] {
            let payload = vec![0x42; length];
            let mut bytes = Fixture::words(&[
                1,
                (44 + gap + length) as u32,
                (36 + gap) as u32,
                length as u32,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
            ]);
            bytes.resize(44 + gap, 0xa5);
            bytes.extend_from_slice(&payload);
            assert_eq!(packet::decode(&bytes).unwrap(), [payload.as_slice()]);
        }
    }
}
