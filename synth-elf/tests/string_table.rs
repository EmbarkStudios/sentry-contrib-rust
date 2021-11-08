use synth_elf::StringTable;

#[test]
fn empty() {
    let empty = StringTable::default().finish().unwrap();

    assert_eq!(empty, &[0]);
}

#[test]
fn basic() {
    let s1 = "table fills with strings";
    let s2 = "offsets preserved as labels";
    let s3 = "verified with tests";

    let mut table = StringTable::default();

    let l1 = table.add(s1);
    let l2 = table.add(s2);
    let l3 = table.add(s3);

    let expected = {
        let mut v = Vec::new();
        v.push(0);
        v.extend_from_slice(s1.as_bytes());
        v.push(0);
        v.extend_from_slice(s2.as_bytes());
        v.push(0);
        v.extend_from_slice(s3.as_bytes());
        v.push(0);

        v
    };

    let contents = table.finish().unwrap();
    assert_eq!(expected, contents);

    assert_eq!(Some(1), l1.value());
    assert_eq!(Some(1 + s1.len() as u64 + 1), l2.value());
    assert_eq!(
        Some(1 + s1.len() as u64 + 1 + s2.len() as u64 + 1),
        l3.value()
    );
}

#[test]
fn duplicates() {
    let s1 = "string 1";
    let s2 = "string 2";
    let s3 = "";

    let mut table = StringTable::default();

    let l1 = table.add(s1);
    let l2 = table.add(s2);

    // These should be the same labels
    let l3 = table.add(s3);
    let l4 = table.add(s2);

    let expected = {
        let mut v = Vec::new();
        v.push(0);
        v.extend_from_slice(s1.as_bytes());
        v.push(0);
        v.extend_from_slice(s2.as_bytes());
        v.push(0);

        v
    };

    let contents = table.finish().unwrap();
    assert_eq!(expected, contents);

    assert_eq!(Some(0), l3.value());
    assert_eq!(Some(1), l1.value());
    assert_eq!(l2.value(), l4.value());
}
