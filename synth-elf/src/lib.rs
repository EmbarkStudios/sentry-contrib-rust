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
        let this = Self {
            section: Section::with_endian(endian),
            strings: std::collections::HashMap::new(),
        };

        this.add("").0
    }

    pub fn add(mut self, string: impl Into<String>) -> (Self, Label) {
        let string = string.into();
        if let Some(label) = self.strings.get(&string).cloned() {
            return (self, label.clone());
        }

        let here = self.section.here();
        self.section = self
            .section
            .append_bytes(string.as_bytes())
            // null terminator
            .append_bytes(&[0]);

        self.strings.insert(string, here.clone());
        (self, here)
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

fn append_num_with_size(section: Section, is_64_bits: bool, num: impl NumCast) -> Section {
    if is_64_bits {
        section.D64(num.to_u64())
    } else {
        section.D32(num.to_u32())
    }
}

fn append_label_with_size(section: Section, is_64_bits: bool, label: &Label) -> Section {
    if is_64_bits {
        section.D64(label)
    } else {
        section.D32(label)
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

        section = section
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

        section = section
            // e_type
            .D16(header::ET_EXEC)
            // e_machine
            .D16(machine)
            // e_version
            .D32(header::EV_CURRENT as u32);

        let program_header_label = Label::new();
        let section_header_label = Label::new();
        let program_count_label = Label::new();
        let section_count_label = Label::new();
        let section_header_string_index = Label::new();

        // e_entry
        section = append_num_with_size(section, file_class.is_64(), 0 as u32);
        // e_phoff
        section = append_label_with_size(section, file_class.is_64(), &program_header_label);
        // e_shoff
        section = append_label_with_size(section, file_class.is_64(), &section_header_label);

        section = section
            // e_flags
            .D32(0)
            // e_ehsize
            .D16(file_class.ehsize())
            // e_phentsize
            .D16(file_class.phentsize())
            // e_phnum
            .D16(program_count_label)
            // e_shentsize
            .D16(file_class.shentsize())
            // e_shnum
            .D16(section_count_label)
            // e_shstrndx
            .D16(section_header_string_index);

        let this = Self {
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
        this.add_section("", Section::new(), elf::section_header::SHT_NULL)
            .0
    }

    /// Add the section to the section header table and append it to the file.
    /// Returns the index of the section in the section header table.
    pub fn add_section(
        self,
        name: impl Into<String>,
        section: Section,
        kind: u32,
    ) -> (Self, usize) {
        self.add_section_with_attrs(name, section, kind, SectionAttrs::default())
    }

    /// Add the section using the specified attributes to the section header
    /// table and append it to the file. Returns the index of the section in the
    /// section header table.
    pub fn add_section_with_attrs(
        mut self,
        name: impl Into<String>,
        section: Section,
        kind: u32,
        attrs: SectionAttrs,
    ) -> (Self, usize) {
        let (shs, string_label) = self.section_header_strings.add(name);

        let size = section.size();

        let mut section_headers = self
            .section_headers
            // sh_name
            .D32(string_label)
            // sh_type
            .D32(kind);

        let is_64_bits = self.addr_size == 8;

        let offset_label = Label::new();

        // sh_flags
        section_headers = append_num_with_size(section_headers, is_64_bits, attrs.flags);
        // sh_addr
        section_headers = append_num_with_size(section_headers, is_64_bits, attrs.addr);
        // sh_offset
        section_headers = append_label_with_size(section_headers, is_64_bits, &offset_label);
        // sh_size
        section_headers = append_num_with_size(section_headers, is_64_bits, size);
        section_headers = section_headers
            // sh_link
            .D32(attrs.link)
            // sh_info
            .D32(0);

        // sh_addralign
        section_headers = append_num_with_size(section_headers, is_64_bits, 0 as u32);
        // sh_entsize
        section_headers = append_num_with_size(section_headers, is_64_bits, attrs.entsize);

        self.sections.push(ElfSection {
            inner: section,
            kind,
            addr: attrs.addr as u32,
            offset: attrs.offset as u32,
            offset_label,
            size: size as u32,
        });

        let index = self.sections.len() - 1;

        (
            Self {
                section: self.section,
                addr_size: self.addr_size,
                program_header_label: self.program_header_label,
                program_count: self.program_count,
                program_count_label: self.program_count_label,
                program_headers: self.program_headers,
                section_header_label: self.section_header_label,
                section_count_label: self.section_count_label,
                section_headers,
                section_header_string_index: self.section_header_string_index,
                section_header_strings: shs,
                sections: self.sections,
            },
            index,
        )
    }

    pub fn add_segment(mut self, 

    /// Finalizes the elf
    pub fn finish(self) -> Option<Vec<u8>> {
        self.section_header_string_index
            .set_const(self.sections.len() as u64);

        let (mut this, _) = self.add_section(
            ".shstrtab",
            self.section_header_strings.section,
            elf::section_header::SHT_STRTAB,
        );

        if this.program_count > 0 {
            this.section = this
                .section
                .mark(&this.program_header_label)
                .append_section(this.program_headers);
        } else {
            this.program_header_label.set_const(0);
        }

        for esec in this.sections {
            // NULL and NOBITS sections have no content, so they don't need to
            // be written to the file.
            if esec.kind == elf::section_header::SHT_NULL {
                esec.offset_label.set_const(0);
            } else if esec.kind == elf::section_header::SHT_NOBITS {
                esec.offset_label.set_const(esec.offset as u64);
            } else {
                this.section = this
                    .section
                    .mark(&esec.offset_label)
                    .append_section(esec.inner)
                    .align(4);
            }
        }

        this.section_count_label
            .set_const(this.sections.len() as u64);
        this.program_count_label
            .set_const(this.program_count as u64);

        let finished = this
            .section
            .mark(&this.section_header_label)
            .append_section(this.section_headers);

        finished.get_contents()
    }
}
