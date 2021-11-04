//! Breakpad has some utilities to synthesize elf binaries for testing purposes
//! so that binary data doesn't need to be checked in, so this is just a port
//! of that for our own tests, but this should ideally be part of another crate,
//! such as [goblin](https://github.com/m4b/goblin/issues/185)

use goblin::elf::{self, header};
pub use test_assembler::{Endian, Label, LabelMaker, Section};

pub struct StringTable {
    section: Section,
    strings: std::collections::HashMap<String, Label>,
}

impl Default for StringTable {
    fn default() -> Self {
        Self::with_endian(test_assembler::DEFAULT_ENDIAN)
    }
}

impl StringTable {
    pub fn with_endian(endian: Endian) -> Self {
        let mut this = Self {
            section: Section::with_endian(endian),
            strings: std::collections::HashMap::new(),
        };

        this.section.set_start_const(0);
        this.add("");
        this
    }

    pub fn add(&mut self, string: impl Into<String>) -> Label {
        let string = string.into();
        if let Some(label) = self.strings.get(&string) {
            return label.clone();
        }

        let here = self.section.here();
        self.section
            .append_bytes(string.as_bytes())
            // null terminator
            .append_bytes(&[0]);

        self.strings.insert(string, here.clone());
        here
    }
}

pub struct ElfSection {
    inner: Section,
    kind: u32,
    addr: u32,
    offset: u32,
    offset_label: Label,
    size: u32,
}

#[derive(Copy, Clone)]
pub enum ElfClass {
    Class32,
    Class64,
}

impl ElfClass {
    pub fn is_64(self) -> bool {
        matches!(self, Self::Class64)
    }

    pub fn class(self) -> u8 {
        match self {
            Self::Class32 => header::ELFCLASS32,
            Self::Class64 => header::ELFCLASS64,
        }
    }

    pub fn addr_size(self) -> usize {
        match self {
            Self::Class32 => 4,
            Self::Class64 => 8,
        }
    }

    pub fn ehsize(self) -> u16 {
        match self {
            Self::Class32 => header::header32::SIZEOF_EHDR as u16,
            Self::Class64 => header::header64::SIZEOF_EHDR as u16,
        }
    }

    pub fn phentsize(self) -> u16 {
        match self {
            Self::Class32 => elf::program_header::program_header32::SIZEOF_PHDR as u16,
            Self::Class64 => elf::program_header::program_header64::SIZEOF_PHDR as u16,
        }
    }

    pub fn shentsize(self) -> u16 {
        match self {
            Self::Class32 => elf::section_header::section_header32::SIZEOF_SHDR as u16,
            Self::Class64 => elf::section_header::section_header64::SIZEOF_SHDR as u16,
        }
    }
}

trait NumCast: test_assembler::Num {
    fn to_u32(self) -> u32;
    fn to_u64(self) -> u64;
}

impl NumCast for u32 {
    fn to_u32(self) -> u32 {
        self
    }
    fn to_u64(self) -> u64 {
        self as u64
    }
}

impl NumCast for u64 {
    fn to_u32(self) -> u32 {
        self as u32
    }
    fn to_u64(self) -> u64 {
        self
    }
}

trait WithSize {
    fn append_word(&mut self, is_64_bits: bool, num: impl NumCast) -> &mut Self;
    fn append_word_label(&mut self, is_64_bits: bool, label: &Label) -> &mut Self;
}

impl WithSize for Section {
    fn append_word(&mut self, is_64_bits: bool, num: impl NumCast) -> &mut Section {
        if is_64_bits {
            self.D64(num.to_u64())
        } else {
            self.D32(num.to_u32())
        }
    }

    fn append_word_label(&mut self, is_64_bits: bool, label: &Label) -> &mut Section {
        if is_64_bits {
            self.D64(label)
        } else {
            self.D32(label)
        }
    }
}

#[derive(Default)]
pub struct SectionAttrs {
    pub flags: u32,
    pub addr: u64,
    pub link: u32,
    pub entsize: u64,
    pub offset: u64,
}

pub struct Elf {
    section: Section,
    addr_size: usize,
    program_header_label: Label,
    program_count: usize,
    program_count_label: Label,
    program_headers: Section,
    section_header_label: Label,
    section_count_label: Label,
    section_headers: Section,
    section_header_string_index: Label,
    section_header_strings: StringTable,
    sections: Vec<ElfSection>,
}

impl Elf {
    pub fn new(machine: u16, file_class: ElfClass, endian: Endian) -> Self {
        let mut section = Section::with_endian(endian);

        section
            .set_start_const(0)
            .append_bytes(header::ELFMAG)
            // ei_class
            .D8(file_class.class())
            // ei_data
            .D8(match endian {
                Endian::Little => header::ELFDATA2LSB,
                Endian::Big => header::ELFDATA2MSB,
            })
            // ei_version
            .D8(header::EV_CURRENT)
            // ei_abiversion
            .D8(header::ELFOSABI_NONE)
            // ei_abiversion
            .D8(0)
            // ei_pad
            .append_repeated(0, 7);

        debug_assert_eq!(section.size() as usize, header::SIZEOF_IDENT);

        let program_header_label = Label::new();
        let section_header_label = Label::new();
        let program_count_label = Label::new();
        let section_count_label = Label::new();
        let section_header_string_index = Label::new();

        let is_64_bits = file_class.is_64();

        section
            // e_type
            .D16(header::ET_EXEC)
            // e_machine
            .D16(machine)
            // e_version
            .D32(header::EV_CURRENT as u32)
            // e_entry
            .append_word(is_64_bits, 0u32)
            // e_phoff
            .append_word_label(is_64_bits, &program_header_label)
            // e_shoff
            .append_word_label(is_64_bits, &section_header_label)
            // e_flags
            .D32(0)
            // e_ehsize
            .D16(file_class.ehsize())
            // e_phentsize
            .D16(file_class.phentsize())
            // e_phnum
            .D16(&program_count_label)
            // e_shentsize
            .D16(file_class.shentsize())
            // e_shnum
            .D16(&section_count_label)
            // e_shstrndx
            .D16(&section_header_string_index);

        let mut this = Self {
            section,
            addr_size: file_class.addr_size(),
            program_header_label,
            program_count: 0,
            program_count_label,
            program_headers: Section::with_endian(endian),
            section_header_label,
            section_count_label,
            section_headers: Section::with_endian(endian),
            section_header_string_index,
            section_header_strings: StringTable::with_endian(endian),
            sections: Vec::new(),
        };

        // Empty section for SHN_UNDEF
        this.add_section("", Section::new(), elf::section_header::SHT_NULL);
        this
    }

    /// Add the section to the section header table and append it to the file.
    /// Returns the index of the section in the section header table.
    pub fn add_section(&mut self, name: impl Into<String>, section: Section, kind: u32) -> usize {
        self.add_section_with_attrs(name, section, kind, SectionAttrs::default())
    }

    /// Add the section using the specified attributes to the section header
    /// table and append it to the file. Returns the index of the section in the
    /// section header table.
    pub fn add_section_with_attrs(
        &mut self,
        name: impl Into<String>,
        section: Section,
        kind: u32,
        attrs: SectionAttrs,
    ) -> usize {
        let string_label = self.section_header_strings.add(name);
        let size = section.size();
        let is_64_bits = self.addr_size == 8;

        let elf_section = Self::do_add_section(
            &mut self.section_headers,
            string_label,
            size,
            is_64_bits,
            section,
            kind,
            attrs,
        );

        self.sections.push(elf_section);
        self.sections.len() - 1
    }

    fn do_add_section(
        section_headers: &mut Section,
        string_label: Label,
        size: u64,
        is_64_bits: bool,
        section: Section,
        kind: u32,
        attrs: SectionAttrs,
    ) -> ElfSection {
        let offset_label = Label::new();

        section_headers
            // sh_name
            .D32(string_label)
            // sh_type
            .D32(kind)
            // sh_flags
            .append_word(is_64_bits, attrs.flags)
            // sh_addr
            .append_word(is_64_bits, attrs.addr);

        section_headers
            // sh_offset
            .append_word_label(is_64_bits, &offset_label)
            // sh_size
            .append_word(is_64_bits, size)
            // sh_link
            .D32(attrs.link)
            // sh_info
            .D32(0)
            // sh_addralign
            .append_word(is_64_bits, 0u32)
            // sh_entsize
            .append_word(is_64_bits, attrs.entsize);

        ElfSection {
            inner: section,
            kind,
            addr: attrs.addr as u32,
            offset: attrs.offset as u32,
            offset_label,
            size: size as u32,
        }
    }

    pub fn add_segment(&mut self, start: usize, end: usize, kind: u32, flags: u32) {
        self.program_count += 1;
        let is_64_bits = self.addr_size == 8;

        // p_type
        self.program_headers.D32(kind);

        if is_64_bits {
            // p_flags
            self.program_headers.D32(flags);
        }

        let mut file_size = 0;
        let mut mem_size = 0;
        let mut prev_was_nobits = false;
        for section in &self.sections[start..end] {
            let mut size = section.size as u64;
            if section.kind != elf::section_header::SHT_NOBITS {
                debug_assert!(!prev_was_nobits);
                size = (size + 3) & !3;
                file_size += size;
            } else {
                prev_was_nobits = true;
            }

            mem_size += size;
        }

        let section = &self.sections[start];

        self.program_headers
            // p_offset
            .append_word_label(is_64_bits, &section.offset_label)
            // p_vaddr
            .append_word(is_64_bits, section.addr)
            // p_paddr
            .append_word(is_64_bits, section.addr)
            // p_filesz
            .append_word(is_64_bits, file_size)
            // p_memsz
            .append_word(is_64_bits, mem_size);

        if !is_64_bits {
            // p_flags
            self.program_headers.D32(flags);
        }

        // p_align
        self.program_headers.append_word(is_64_bits, 0u32);
    }

    /// Finalizes the elf
    pub fn finish(mut self) -> Option<Vec<u8>> {
        self.section_header_string_index
            .set_const(self.sections.len() as u64);

        {
            let string_label = self.section_header_strings.add(".shstrtab");
            let size = self.section.size();
            let is_64_bits = self.addr_size == 8;

            let elf_section = Self::do_add_section(
                &mut self.section_headers,
                string_label,
                size,
                is_64_bits,
                self.section_header_strings.section,
                elf::section_header::SHT_STRTAB,
                SectionAttrs::default(),
            );

            self.sections.push(elf_section);
        }

        if self.program_count > 0 {
            self.section
                .mark(&self.program_header_label)
                .append_section(self.program_headers);
        } else {
            self.program_header_label.set_const(0);
        }

        let num_sections = self.sections.len() as u64;
        for esec in self.sections {
            // NULL and NOBITS sections have no content, so they don't need to
            // be written to the file.
            if esec.kind == elf::section_header::SHT_NULL {
                esec.offset_label.set_const(0);
            } else if esec.kind == elf::section_header::SHT_NOBITS {
                esec.offset_label.set_const(esec.offset as u64);
            } else {
                self.section.mark(&esec.offset_label);

                self.section.append_section(esec.inner).align(4);
            }
        }

        self.section_count_label.set_const(num_sections);
        self.program_count_label
            .set_const(self.program_count as u64);

        self.section
            .mark(&self.section_header_label)
            .append_section(self.section_headers);

        self.section.get_contents()
    }
}
