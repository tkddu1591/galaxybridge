use galaxybridge::usb::layout::{CONFIGURATION_LIMIT, Layout};
use std::collections::BTreeSet;

struct Fixture;
impl Fixture {
    fn records() -> Vec<Vec<u8>> {
        vec![
            vec![8, 11, 0, 2, 0xe0, 1, 3, 0],
            vec![9, 4, 0, 0, 1, 0xe0, 1, 3, 0],
            vec![5, 0x24, 0, 0x10, 1],
            vec![5, 0x24, 6, 0, 1],
            vec![7, 5, 0x81, 3, 8, 0, 9],
            vec![9, 4, 1, 0, 2, 0x0a, 0, 0, 0],
            vec![7, 5, 0x82, 2, 0, 2, 0],
            vec![7, 5, 3, 2, 0, 2, 0],
        ]
    }
    fn bytes(records: &[Vec<u8>]) -> Vec<u8> {
        let count = records
            .iter()
            .filter(|r| r.len() >= 9 && r[1] == 4)
            .map(|r| r[2])
            .collect::<BTreeSet<_>>()
            .len();
        let mut bytes = vec![9, 2, 0, 0, count as u8, 1, 0, 0x80, 50];
        bytes.extend(records.iter().flatten());
        let length = u16::try_from(bytes.len()).unwrap();
        bytes[2..4].copy_from_slice(&length.to_le_bytes());
        bytes
    }
}

#[test]
fn selects_explicit_union_or_conventional_adjacent_function() {
    for omit in [None, Some(0), Some(3)] {
        let mut records = Fixture::records();
        if let Some(index) = omit {
            records.remove(index);
        }
        let layout = Layout::parse(&Fixture::bytes(&records)).unwrap();
        assert_eq!((layout.control.number, layout.control.alternate), (0, 0));
        assert_eq!((layout.data.number, layout.data.alternate), (1, 0));
        assert_eq!(
            (layout.notification, layout.incoming, layout.outgoing),
            (0x81, 0x82, 3)
        );
    }
}

#[test]
fn accepts_one_usable_data_alternate_but_rejects_two() {
    let mut records = Fixture::records();
    records[5][3] = 1;
    records.insert(5, vec![9, 4, 1, 0, 0, 0x0a, 0, 0, 0]);
    assert_eq!(
        Layout::parse(&Fixture::bytes(&records))
            .unwrap()
            .data
            .alternate,
        1
    );
    let mut extra = records[6..9].to_vec();
    extra[0][3] = 2;
    records.extend(extra);
    assert!(Layout::parse(&Fixture::bytes(&records)).is_err());
}

#[test]
fn malformed_or_duplicate_unions_never_fall_back_to_adjacency() {
    for union in [
        vec![4, 0x24, 6, 0],
        vec![6, 0x24, 6, 0, 1, 2],
        vec![5, 0x24, 6, 1, 1],
        vec![5, 0x24, 6, 0, 0],
        vec![5, 0x24, 6, 0, 2],
    ] {
        let mut records = Fixture::records();
        records[3] = union;
        assert!(Layout::parse(&Fixture::bytes(&records)).is_err());
    }
    let mut records = Fixture::records();
    records.insert(3, records[3].clone());
    assert!(Layout::parse(&Fixture::bytes(&records)).is_err());
}

#[test]
fn associations_cannot_overlap_or_assign_data_to_another_function() {
    for association in [
        vec![8, 11, 0, 1, 0xe0, 1, 3, 0],
        vec![8, 11, 1, 1, 0xe0, 1, 3, 0],
        vec![8, 11, 0, 0, 0xe0, 1, 3, 0],
        vec![8, 11, 255, 2, 0xe0, 1, 3, 0],
    ] {
        let mut records = Fixture::records();
        records[0] = association;
        assert!(Layout::parse(&Fixture::bytes(&records)).is_err());
    }
    let mut records = Fixture::records();
    records.insert(0, records[0].clone());
    assert!(Layout::parse(&Fixture::bytes(&records)).is_err());
}

#[test]
fn endpoint_type_direction_address_and_packet_limits_are_checked() {
    for (record, field, value) in [
        (4, 2, 0x01), // notification must be IN
        (4, 3, 2),    // notification must be interrupt
        (4, 4, 0),    // notification must fit eight bytes
        (4, 5, 0x80), // reserved maximum packet bits
        (4, 6, 0),    // invalid interrupt interval
        (6, 2, 0x80), // endpoint zero
        (6, 2, 0x92), // reserved address bits
        (6, 2, 0x81), // conflicts with control endpoint
        (6, 3, 3),    // data must be bulk
        (6, 5, 0),    // zero packet size
        (6, 5, 0x0a), // invalid high-bandwidth bulk flags
        (7, 2, 0x83), // two incoming, no outgoing endpoint
    ] {
        let mut records = Fixture::records();
        records[record][field] = value;
        assert!(
            Layout::parse(&Fixture::bytes(&records)).is_err(),
            "record {record}, field {field}"
        );
    }
}

#[test]
fn duplicate_endpoints_alternates_and_notification_choices_are_rejected() {
    let mut records = Fixture::records();
    records[5][4] = 3;
    records.push(records[6].clone());
    assert!(Layout::parse(&Fixture::bytes(&records)).is_err());

    let mut records = Fixture::records();
    records.extend(records[5..8].to_vec());
    assert!(Layout::parse(&Fixture::bytes(&records)).is_err());

    let mut records = Fixture::records();
    records[1][4] = 2;
    records.insert(5, vec![7, 5, 0x84, 3, 8, 0, 9]);
    assert!(Layout::parse(&Fixture::bytes(&records)).is_err());
}

#[test]
fn malformed_descriptor_tails_cannot_hide_additional_interfaces() {
    for tail in [
        vec![0, 0],
        vec![1, 4],
        vec![9, 4],
        vec![3, 0x24],
        vec![2, 4],
        vec![2, 5],
    ] {
        let mut records = Fixture::records();
        records.push(tail);
        assert!(Layout::parse(&Fixture::bytes(&records)).is_err());
    }
    let bytes = Fixture::bytes(&Fixture::records());
    for end in 0..bytes.len() {
        assert!(Layout::parse(&bytes[..end]).is_err());
    }
    let mut longer = bytes.clone();
    longer.push(0);
    assert!(Layout::parse(&longer).is_err());
    assert!(Layout::parse(&vec![0; CONFIGURATION_LIMIT + 1]).is_err());
}

#[test]
fn every_single_bit_descriptor_mutation_remains_bounded() {
    let original = Fixture::bytes(&Fixture::records());
    for position in 0..original.len() {
        for bit in 0..8 {
            let mut bytes = original.clone();
            bytes[position] ^= 1 << bit;
            if let Ok(layout) = Layout::parse(&bytes) {
                assert_ne!(layout.control.number, layout.data.number);
                assert_ne!(layout.incoming, layout.notification);
                assert_eq!(layout.incoming & 0x80, 0x80);
                assert_eq!(layout.outgoing & 0x80, 0);
            }
        }
    }
}
