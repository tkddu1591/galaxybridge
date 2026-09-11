#![no_main]
use galaxybridge::usb::layout::{CONFIGURATION_LIMIT, Layout};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    if let Ok(layout) = Layout::parse(bytes) {
        assert!(bytes.len() <= CONFIGURATION_LIMIT);
        assert_eq!(
            u16::from_le_bytes(bytes[2..4].try_into().unwrap()) as usize,
            bytes.len()
        );
        assert_ne!(layout.control.number, layout.data.number);
        assert_ne!(layout.notification, layout.incoming);
        assert_eq!(layout.notification & 0x80, 0x80);
        assert_eq!(layout.incoming & 0x80, 0x80);
        assert_eq!(layout.outgoing & 0x80, 0);
        for endpoint in [layout.notification, layout.incoming, layout.outgoing] {
            assert_eq!(endpoint & 0x70, 0);
            assert_ne!(endpoint & 0x0f, 0);
        }
        // Independently walk every record to check that successful selection
        // never masks a truncated/zero-length tail or duplicate selected alt.
        let mut control_count = 0;
        let mut data_count = 0;
        let mut rest = bytes;
        while !rest.is_empty() {
            assert!(rest.len() >= 2);
            let length = rest[0] as usize;
            assert!(length >= 2 && length <= rest.len());
            if rest[1] == 4 {
                assert!(length >= 9);
                if rest[2] == layout.control.number && rest[3] == layout.control.alternate {
                    control_count += 1;
                }
                if rest[2] == layout.data.number && rest[3] == layout.data.alternate {
                    data_count += 1;
                }
            }
            rest = &rest[length..];
        }
        assert_eq!(control_count, 1);
        assert_eq!(data_count, 1);
    }
});
