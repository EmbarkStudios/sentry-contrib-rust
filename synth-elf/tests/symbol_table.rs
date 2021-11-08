use synth_elf::{Endian, StInfo, StringTable, SymbolTable};

#[test]
fn simple_32() {
    let mut st = StringTable::default();
    let mut symtab = SymbolTable::<u32>::with_endian(Endian::Little);

    #[derive(Copy, Clone)]
    struct Func {
        name: &'static str,
        addr: u32,
        size: u32,
    }

    let funcs = [
        Func {
            name: "superfunc",
            addr: 0x10001000,
            size: 0x10,
        },
        Func {
            name: "awesomefunc",
            addr: 0x20002000,
            size: 0x2f,
        },
        Func {
            name: "megafunc",
            addr: 0x30003000,
            size: 0x3c,
        },
    ];

    for (i, func) in funcs.into_iter().enumerate() {
        symtab.add_symbol(
            &mut st,
            func.name,
            func.addr,
            func.size,
            StInfo {
                bind: if i == 0 {
                    goblin::elf::sym::STB_GLOBAL
                } else {
                    goblin::elf::sym::STB_LOCAL
                },
                kind: goblin::elf::sym::STT_FUNC,
            },
            goblin::elf::section_header::SHN_UNDEF as u16 + i as u16 + 1,
        );
    }

    {
        let expected = {
            let mut v = Vec::new();
            v.push(0);

            for func in funcs {
                v.extend_from_slice(func.name.as_bytes());
                v.push(0);
            }

            v
        };
        let st_contents = st.finish().unwrap();

        assert_eq!(expected, st_contents);
    }

    #[rustfmt::skip]
    let expected = vec![
        // superfunc
        0x01, 0, 0, 0, // name
        0, 0x10, 0, 0x10, // value
        0x10, 0, 0, 0, // size
        StInfo { bind: goblin::elf::sym::STB_GLOBAL, kind: goblin::elf::sym::STT_FUNC }.into(), // info
        0, // other
        0x01, 0, // shndx
        // awesomefunc
        0x0b, 0, 0, 0, // name
        0, 0x20, 0, 0x20, // value
        0x2f, 0, 0, 0, // size
        StInfo { bind: goblin::elf::sym::STB_LOCAL, kind: goblin::elf::sym::STT_FUNC }.into(), // info
        0, // other
        0x02, 0, // shndx
        // megafunc
        0x17, 0, 0, 0, // name
        0, 0x30, 0, 0x30, // value
        0x3c, 0, 0, 0, // size
        StInfo { bind: goblin::elf::sym::STB_GLOBAL, kind: goblin::elf::sym::STT_FUNC }.into(), // info
        0, // other
        0x03, 0, // shndx
    ];

    assert_eq!(expected, symtab.finish().unwrap());
}
