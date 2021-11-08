use goblin::elf::{self, header as hdr};
use scroll::ctx::TryFromCtx;
use synth_elf::{Elf, ElfClass, Endian, Section};

fn empty_le(class: ElfClass) {
    let contents = Elf::new(hdr::EM_386, class, Endian::Little)
        .finish()
        .unwrap();

    let expected_st = b"\0.shstrtab\0";
    let st_align = 4 - expected_st.len() % 4;

    assert_eq!(
        contents.len(),
        // Elf Header
        class.ehsize() as usize +
        // 2 sections, the NULL and STRTAB
        2 * class.shentsize() as usize +
        expected_st.len() + st_align,
    );

    // Elf header
    let sh_off = {
        let (header, header_size) =
            elf::Header::try_from_ctx(&contents, goblin::container::Endian::Little).unwrap();

        assert_eq!(
            header.e_ident,
            [
                hdr::ELFMAG[0],
                hdr::ELFMAG[1],
                hdr::ELFMAG[2],
                hdr::ELFMAG[3],
                class.class(),
                hdr::ELFDATA2LSB,
                hdr::EV_CURRENT,
                hdr::ELFOSABI_NONE,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0
            ]
        );

        assert_eq!(header.e_type, hdr::ET_EXEC);
        assert_eq!(header.e_machine, hdr::EM_386);
        assert_eq!(header.e_version, hdr::EV_CURRENT as u32);
        assert_eq!(header.e_entry, 0);
        assert_eq!(header.e_phoff, 0);
        assert_eq!(
            header.e_shoff as usize,
            header_size + expected_st.len() + st_align
        );
        assert_eq!(header.e_flags, 0);
        assert_eq!(header.e_ehsize, header_size as u16);
        assert_eq!(header.e_phentsize, class.phentsize());
        assert_eq!(header.e_phnum, 0);
        assert_eq!(header.e_shentsize, class.shentsize());
        assert_eq!(header.e_shnum, 2);
        assert_eq!(header.e_shstrndx, 1);

        header.e_shoff
    };

    let section_headers = elf::section_header::SectionHeader::parse(
        &contents,
        sh_off as usize,
        2,
        goblin::container::Ctx {
            container: if class.is_64() {
                goblin::container::Container::Big
            } else {
                goblin::container::Container::Little
            },
            le: goblin::container::Endian::Little,
        },
    )
    .unwrap();

    // SHN_UNDEF
    {
        let section = &section_headers[0];

        assert_eq!(section.sh_name, 0);
        assert_eq!(section.sh_type, elf::section_header::SHT_NULL);
        assert_eq!(section.sh_flags, 0);
        assert_eq!(section.sh_addr, 0);
        assert_eq!(section.sh_offset, 0);
        assert_eq!(section.sh_size, 0);
        assert_eq!(section.sh_link, 0);
        assert_eq!(section.sh_info, 0);
        assert_eq!(section.sh_addralign, 0);
        assert_eq!(section.sh_entsize, 0);
    }

    // .shstrtab
    {
        let section = &section_headers[1];

        assert_eq!(section.sh_name, 1);
        assert_eq!(section.sh_type, elf::section_header::SHT_STRTAB);
        assert_eq!(section.sh_flags, 0);
        assert_eq!(section.sh_addr, 0);
        assert_eq!(section.sh_offset, class.ehsize() as u64);
        assert_eq!(section.sh_size, expected_st.len() as u64);
        assert_eq!(section.sh_link, 0);
        assert_eq!(section.sh_info, 0);
        assert_eq!(section.sh_addralign, 0);
        assert_eq!(section.sh_entsize, 0);
    }
}

fn basic_le(class: ElfClass) {
    let contents = {
        let mut elf = Elf::new(hdr::EM_386, class, Endian::Little);

        let text = elf.add_section(
            ".text",
            Section::inline(Some(Endian::Little), |s| s.append_repeated(0, 4 * 1024)),
            elf::section_header::SHT_PROGBITS,
        );
        let bss = elf.add_section(
            ".bss",
            Section::inline(Some(Endian::Little), |s| s.append_repeated(0, 16)),
            elf::section_header::SHT_NOBITS,
        );

        elf.add_segment(text, bss, elf::program_header::PT_LOAD, 0);
        elf.finish().unwrap()
    };

    let expected_st = b"\0.text\0.bss\0.shstrtab\0";
    let st_align = 4 - expected_st.len() % 4;

    assert_eq!(
        contents.len(),
        // Elf Header
        class.ehsize() as usize +
        // 4 sections, NULL, STRTAB, text, and program header
        4 * class.shentsize() as usize + class.phentsize() as usize + 4 * 1024 +
        expected_st.len() + st_align,
    );

    // Elf header
    let (sh_off, ph_off) = {
        let (header, header_size) =
            elf::Header::try_from_ctx(&contents, goblin::container::Endian::Little).unwrap();

        assert_eq!(
            header.e_ident,
            [
                hdr::ELFMAG[0],
                hdr::ELFMAG[1],
                hdr::ELFMAG[2],
                hdr::ELFMAG[3],
                class.class(),
                hdr::ELFDATA2LSB,
                hdr::EV_CURRENT,
                hdr::ELFOSABI_NONE,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0
            ]
        );

        assert_eq!(header.e_type, hdr::ET_EXEC);
        assert_eq!(header.e_machine, hdr::EM_386);
        assert_eq!(header.e_version, hdr::EV_CURRENT as u32);
        assert_eq!(header.e_entry, 0);
        assert_eq!(header.e_phoff as u16, class.ehsize());
        assert_eq!(
            header.e_shoff as usize,
            header_size + class.phentsize() as usize + 4 * 1024 + expected_st.len() + st_align
        );
        assert_eq!(header.e_flags, 0);
        assert_eq!(header.e_ehsize, header_size as u16);
        assert_eq!(header.e_phentsize, class.phentsize());
        assert_eq!(header.e_phnum, 1);
        assert_eq!(header.e_shentsize, class.shentsize());
        assert_eq!(header.e_shnum, 4);
        assert_eq!(header.e_shstrndx, 3);

        (header.e_shoff, header.e_phoff)
    };

    let section_headers = elf::section_header::SectionHeader::parse(
        &contents,
        sh_off as usize,
        4,
        goblin::container::Ctx {
            container: if class.is_64() {
                goblin::container::Container::Big
            } else {
                goblin::container::Container::Little
            },
            le: goblin::container::Endian::Little,
        },
    )
    .unwrap();

    // SHN_UNDEF
    {
        let section = &section_headers[0];

        assert_eq!(section.sh_name, 0);
        assert_eq!(section.sh_type, elf::section_header::SHT_NULL);
        assert_eq!(section.sh_flags, 0);
        assert_eq!(section.sh_addr, 0);
        assert_eq!(section.sh_offset, 0);
        assert_eq!(section.sh_size, 0);
        assert_eq!(section.sh_link, 0);
        assert_eq!(section.sh_info, 0);
        assert_eq!(section.sh_addralign, 0);
        assert_eq!(section.sh_entsize, 0);
    }

    // .text
    {
        let section = &section_headers[1];

        assert_eq!(section.sh_name, 1);
        assert_eq!(section.sh_type, elf::section_header::SHT_PROGBITS);
        assert_eq!(section.sh_flags, 0);
        assert_eq!(section.sh_addr, 0);
        assert_eq!(section.sh_offset as u16, class.ehsize() + class.phentsize());
        assert_eq!(section.sh_size, 4 * 1024);
        assert_eq!(section.sh_link, 0);
        assert_eq!(section.sh_info, 0);
        assert_eq!(section.sh_addralign, 0);
        assert_eq!(section.sh_entsize, 0);
    }

    // .bss
    {
        let section = &section_headers[2];

        assert_eq!(section.sh_name, b"\0.text\0".len());
        assert_eq!(section.sh_type, elf::section_header::SHT_NOBITS);
        assert_eq!(section.sh_flags, 0);
        assert_eq!(section.sh_addr, 0);
        assert_eq!(section.sh_offset, 0);
        assert_eq!(section.sh_size, 16);
        assert_eq!(section.sh_link, 0);
        assert_eq!(section.sh_info, 0);
        assert_eq!(section.sh_addralign, 0);
        assert_eq!(section.sh_entsize, 0);
    }

    // .shstrtab
    {
        let section = &section_headers[3];

        assert_eq!(section.sh_name, b"\0.text\0.bss\0".len());
        assert_eq!(section.sh_type, elf::section_header::SHT_STRTAB);
        assert_eq!(section.sh_flags, 0);
        assert_eq!(section.sh_addr, 0);
        assert_eq!(
            section.sh_offset as u16,
            class.ehsize() + class.phentsize() + 4 * 1024
        );
        assert_eq!(section.sh_size, expected_st.len() as u64);
        assert_eq!(section.sh_link, 0);
        assert_eq!(section.sh_info, 0);
        assert_eq!(section.sh_addralign, 0);
        assert_eq!(section.sh_entsize, 0);
    }

    // PT_LOAD
    {
        let program_headers = elf::program_header::ProgramHeader::parse(
            &contents,
            ph_off as usize,
            1,
            goblin::container::Ctx {
                container: if class.is_64() {
                    goblin::container::Container::Big
                } else {
                    goblin::container::Container::Little
                },
                le: goblin::container::Endian::Little,
            },
        )
        .unwrap();

        let ph = &program_headers[0];

        assert_eq!(ph.p_type, elf::program_header::PT_LOAD);
        assert_eq!(ph.p_offset as u16, class.ehsize() + class.phentsize());
        assert_eq!(ph.p_vaddr, 0);
        assert_eq!(ph.p_paddr, 0);
        assert_eq!(ph.p_filesz, 4 * 1024);
        assert_eq!(ph.p_memsz, 4 * 1024 + 16);
        assert_eq!(ph.p_flags, 0);
        assert_eq!(ph.p_align, 0);
    }
}

#[test]
fn empty_le_32() {
    empty_le(ElfClass::Class32);
}

#[test]
fn empty_le_64() {
    empty_le(ElfClass::Class64);
}

#[test]
fn basic_le_32() {
    basic_le(ElfClass::Class32);
}

#[test]
fn basic_le_64() {
    basic_le(ElfClass::Class64);
}
