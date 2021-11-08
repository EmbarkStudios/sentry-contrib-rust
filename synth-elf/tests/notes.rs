use synth_elf::{Endian, Notes};

#[test]
fn empty() {
    let contents = Notes::with_endian(Endian::Little).finish().unwrap();
    assert!(contents.is_empty());
}

#[test]
fn basic() {
    let mut notes = Notes::with_endian(Endian::Little);

    notes
        .add_note(1, "Linux", &[0x42, 0x2, 0, 0])
        .add_note(2, "a", b"foobar");

    let contents = notes.finish().unwrap();

    let expected = {
        let mut v = Vec::new();

        // Note 1
        {
            v.extend_from_slice(&6u32.to_le_bytes());
            v.extend_from_slice(&4u32.to_le_bytes());
            v.extend_from_slice(&1u32.to_le_bytes());
            v.extend_from_slice(b"Linux");
            v.extend_from_slice(&[0u8; 3]);
            v.extend_from_slice(&[0x42, 0x2, 0, 0]);
        }

        // Note 2
        {
            v.extend_from_slice(&2u32.to_le_bytes());
            v.extend_from_slice(&6u32.to_le_bytes());
            v.extend_from_slice(&2u32.to_le_bytes());
            v.extend_from_slice(b"a");
            v.extend_from_slice(&[0u8; 3]);
            v.extend_from_slice(b"foobar");
            v.extend_from_slice(&[0u8; 2]);
        }

        v
    };

    assert_eq!(contents, expected);
}
