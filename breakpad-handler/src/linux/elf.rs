#[derive(Debug)]
enum ElfClass {
    Class32(goblin::elf32::header::Header),
    Class64(goblin::elf64::header::Header),
}

struct MappedElf<'elf> {
    /// The actual byte buffer we are working against
    data: &'elf [u8],
    class: ElfClass,
}

impl<'elf> MappedElf<'elf> {
    fn read(data: &'elf [u8]) -> Option<Self> {
        // Check that this is actually a valid elf
        if &data[..4] != goblin::elf::header::ELFMAG {
            return None;
        }

        let class = dbg!(*data.get(4)?);

        fn parse_header<H: Sized + Copy>(data: &[u8], size: usize) -> Option<H> {
            if data.len() < size {
                return None;
            }

            Some(unsafe { *data.as_ptr().cast::<H>() })
        }

        let class = match class {
            goblin::elf::header::ELFCLASS32 => {
                ElfClass::Class32(parse_header(data, goblin::elf32::header::SIZEOF_EHDR)?)
            }
            goblin::elf::header::ELFCLASS64 => {
                ElfClass::Class64(parse_header(data, goblin::elf64::header::SIZEOF_EHDR)?)
            }
            _ => return None,
        };

        Some(Self { data, class })
    }

    fn find_section_by_name(&self, name: &str, kind: u32) -> Option<&'elf [u8]> {
        macro_rules! find_section {
            ($header:expr, $section_header:ty) => {{
                if $header.e_shoff == 0 {
                    return None;
                }

                let section_headers: &[$section_header] = unsafe {
                    std::slice::from_raw_parts(
                        self.data.as_ptr().offset($header.e_shoff as isize).cast(),
                        $header.e_shnum as usize,
                    )
                };

                let names_section = &section_headers[$header.e_shstrndx as usize];
                let names = &self.data[names_section.sh_offset as usize
                    ..names_section.sh_offset as usize + names_section.sh_size as usize];

                let name = name.as_bytes();

                for sh in section_headers {
                    let name_end = sh.sh_name as usize + name.len();
                    if name_end > names.len() {
                        continue;
                    }

                    let section_name = &names[sh.sh_name as usize..name_end];
                    if sh.sh_type == kind && name == section_name {
                        return Some(
                            &self.data[sh.sh_offset as usize
                                ..sh.sh_offset as usize + sh.sh_size as usize],
                        );
                    }
                }

                None
            }};
        }

        match self.class {
            ElfClass::Class32(hdr) => {
                find_section!(hdr, goblin::elf32::section_header::SectionHeader)
            }
            ElfClass::Class64(hdr) => {
                find_section!(hdr, goblin::elf64::section_header::SectionHeader)
            }
        }
    }

    fn iter_segments(&self, kind: u32) -> impl Iterator<Item = &'elf [u8]> {
        // We need to create our own concrete iterator, otherwise even things
        // like chunkexactiterator have their own types that diverge due to
        // different sizes
        struct PHIter<'elf> {
            ph_headers: &'elf [u8],
            data: &'elf [u8],
            kind: u32,
            count: usize,
            is_64: bool,
            index: usize,
        }

        trait ProgramHeader: Sized {
            fn kind(&self) -> u32;
            fn offset(&self) -> usize;
            fn size(&self) -> usize;
        }

        impl ProgramHeader for goblin::elf32::program_header::ProgramHeader {
            fn kind(&self) -> u32 {
                self.p_type
            }
            fn offset(&self) -> usize {
                self.p_offset as usize
            }
            fn size(&self) -> usize {
                self.p_filesz as usize
            }
        }

        impl ProgramHeader for goblin::elf64::program_header::ProgramHeader {
            fn kind(&self) -> u32 {
                self.p_type
            }
            fn offset(&self) -> usize {
                self.p_offset as usize
            }
            fn size(&self) -> usize {
                self.p_filesz as usize
            }
        }

        impl<'elf> Iterator for PHIter<'elf> {
            type Item = &'elf [u8];

            fn next(&mut self) -> Option<Self::Item> {
                fn imp<'elf, PH: ProgramHeader>(this: &mut PHIter<'elf>) -> Option<&'elf [u8]> {
                    let headers: &[PH] = unsafe {
                        std::slice::from_raw_parts(this.ph_headers.as_ptr().cast(), this.count)
                    };

                    loop {
                        if this.index >= headers.len() {
                            return None;
                        }

                        if dbg!(headers[this.index].kind()) == dbg!(this.kind) {
                            let hdr = &headers[this.index];
                            this.index += 1;

                            return Some(&this.data[hdr.offset()..hdr.offset() + hdr.size()]);
                        }

                        this.index += 1;
                    }
                }

                if self.is_64 {
                    imp::<goblin::elf64::program_header::ProgramHeader>(self)
                } else {
                    imp::<goblin::elf32::program_header::ProgramHeader>(self)
                }
            }
        }

        match self.class {
            ElfClass::Class32(hdr) => PHIter {
                ph_headers: &self.data[hdr.e_phoff as usize
                    ..hdr.e_phoff as usize
                        + std::mem::size_of::<goblin::elf32::program_header::ProgramHeader>()
                            * hdr.e_phnum as usize],
                data: self.data,
                kind,
                count: hdr.e_phnum as usize,
                is_64: false,
                index: 0,
            },
            ElfClass::Class64(hdr) => PHIter {
                ph_headers: &self.data[hdr.e_phoff as usize
                    ..hdr.e_phoff as usize
                        + std::mem::size_of::<goblin::elf64::program_header::ProgramHeader>()
                            * hdr.e_phnum as usize],
                data: self.data,
                kind,
                count: hdr.e_phnum as usize,
                is_64: true,
                index: 0,
            },
        }
    }
}

const MAX_ID_SIZE: usize = 64;

pub struct ElfId {
    // Both ld (and gold) and lld allow the user to specify how they want the
    // build-id written. ld now defaults to sha1 (20 bytes), and lld defaults
    // to `fast` which is actually using xxhash64. However, both also allow
    // user-specified hex-strings, which I assume can be arbitrarily large.
    // But that use case is (I hope) fairly niche, but just in case we give
    // 64 bytes to play with. If someone wants to use identifiers larger than
    // this, they can file a PR to expand, or fallback to a pagevec
    id: [u8; MAX_ID_SIZE],
    len: usize,
}

use std::fmt::{self, Write};

impl fmt::Display for ElfId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", UpperHex(&self.id[..self.len]))
    }
}

pub struct UpperHex<'buff>(&'buff [u8]);

impl<'buff> fmt::Display for UpperHex<'buff> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const CHARS: &[u8] = b"0123456789ABCDEF";

        for &byte in self.0 {
            f.write_char(CHARS[(byte >> 4) as usize] as char)?;
            f.write_char(CHARS[(byte & 0xf) as usize] as char)?;
        }

        Ok(())
    }
}

impl ElfId {
    fn new(slice: &[u8]) -> Option<Self> {
        (slice.len() <= MAX_ID_SIZE).then(|| {
            let mut id = [0u8; MAX_ID_SIZE];

            id[..slice.len()].copy_from_slice(slice);

            Self {
                id,
                len: slice.len(),
            }
        })
    }

    pub fn from_mapped_file(elf: &[u8]) -> Option<Self> {
        let melf = MappedElf::read(elf).unwrap();

        // Attempt to lookup the build-id embedded by the linker, but if no
        // build id is found, fallback to hashing the .text section

        // lld normally creates 2 PT_NOTEs, ld/gold normally creates 1.
        for note in melf.iter_segments(goblin::elf::program_header::PT_NOTE) {
            if let Some(elf_id) = build_id_from_note(note) {
                return Some(elf_id);
            }
        }

        if let Some(elf_id) = melf
            .find_section_by_name(".note.gnu.build-id", goblin::elf::section_header::SHT_NOTE)
            .and_then(|id_sec| build_id_from_note(id_sec))
        {
            return Some(elf_id);
        }

        hash_text_section(&melf)
    }

    /// Converts this identifier into a UUID string with all uppercases. If the
    /// identifier is longer than a 16-byte UUID it will be truncated.
    pub fn as_uuid_string(&self) -> String {
        let mut uuid = [0u8; 16];

        unsafe {
            let to_copy = std::cmp::min(16, self.len);

            let mut ind = 0;

            if ind + 4 <= to_copy {
                let mut part = [0u8; 4];
                part[..4].copy_from_slice(&self.id[ind..ind + 4]);
                part = u32::to_be_bytes(u32::from_ne_bytes(part));
                uuid[ind..ind + 4].copy_from_slice(&part);
                ind += 4;
            }

            if ind + 2 <= to_copy {
                let mut part = [0u8; 2];
                part[..2].copy_from_slice(&self.id[ind..ind + 2]);
                part = u16::to_be_bytes(u16::from_ne_bytes(part));
                uuid[ind..ind + 2].copy_from_slice(&part);
                ind += 2;
            }

            if ind + 2 <= to_copy {
                let mut part = [0u8; 2];
                part[..2].copy_from_slice(&self.id[ind..ind + 2]);
                part = u16::to_be_bytes(u16::from_ne_bytes(part));
                uuid[ind..ind + 2].copy_from_slice(&part);
                ind += 2;
            }

            uuid[ind..to_copy].copy_from_slice(&self.id[ind..to_copy]);
        }

        Self::to_hex_string(&uuid)
    }

    pub fn to_hex_string(bytes: &[u8]) -> String {
        const CHARS: &[u8] = b"0123456789ABCDEF";
        let mut output = String::with_capacity(bytes.len() * 2);

        for &byte in bytes {
            output.push(CHARS[(byte >> 4) as usize] as char);
            output.push(CHARS[(byte & 0xf) as usize] as char);
        }

        output
    }
}

impl AsRef<[u8]> for ElfId {
    fn as_ref(&self) -> &[u8] {
        &self.id[..self.len]
    }
}

fn build_id_from_note(note_section: &[u8]) -> Option<ElfId> {
    use scroll::Pread;

    // goblin "incorrectlY" gates the Pread implementation for the note structs
    // behind the `alloc` feature even though pread doesn't allocate, so we
    // just make our own.
    struct ElfNote<'buffer> {
        kind: u32,
        description: &'buffer [u8],
    }

    impl<'buffer> scroll::ctx::TryFromCtx<'buffer, scroll::Endian> for ElfNote<'buffer> {
        type Error = scroll::Error;

        fn try_from_ctx(
            this: &'buffer [u8],
            le: scroll::Endian,
        ) -> Result<(Self, usize), Self::Error> {
            let offset = &mut 0;

            // Note strings are always 32-bit word aligned
            let align = |offset: &mut usize| {
                let diff = *offset % 4;
                if diff != 0 {
                    *offset += 4 - diff;
                }
            };

            // Notes always use 32-bit words for each field even on 64-bit architectures
            // Length of the note's name, including null terminator
            let name_size = this.gread_with::<u32>(offset, le)?;
            // Length of the note's description, including null terminator
            let desc_size = this.gread_with::<u32>(offset, le)?;
            // The note type
            let kind = this.gread_with::<u32>(offset, le)?;

            // Just skip the name, we don't care
            *offset += name_size as usize;
            align(offset);

            let description = this.gread_with::<&'buffer [u8]>(offset, desc_size as usize)?;
            align(offset);

            Ok((Self { kind, description }, *offset))
        }
    }

    let offset = &mut 0;
    while let Ok(note) = note_section.gread::<ElfNote>(offset) {
        if note.kind == goblin::elf::note::NT_GNU_BUILD_ID {
            if let Some(elf_id) = ElfId::new(note.description) {
                return Some(elf_id);
            }
        }
    }

    None
}

fn hash_text_section(melf: &MappedElf<'_>) -> Option<ElfId> {
    let text_section = melf
        .find_section_by_name(".text", goblin::elf::section_header::SHT_PROGBITS)
        .unwrap();

    // Breakpad limits this to 16-bytes (GUID-ish) size for backwards compat, so
    // we do the same, not that this method should really ever be used in practice
    // since stripping out build ids is not a good idea
    let mut identifier = [0u8; 16];

    // Breakpad hard codes the page size 4k, so just do the same, again for
    // backwards compat
    let first_page = &text_section[..std::cmp::min(text_section.len(), 4 * 1024)];

    // This intentionally disregards the end chunk if we happen to have a text
    // section length < 4k which isn't 16-byte aligned
    for chunk in first_page.chunks_exact(16) {
        for (id, ts) in identifier.iter_mut().zip(chunk.iter()) {
            *id ^= *ts;
        }
    }

    ElfId::new(&identifier)
}

#[cfg(test)]
mod test {
    use super::*;
    use goblin::elf;
    use rstest::{self, *};
    use rstest_reuse::{self, *};
    use synth_elf::{ElfClass, Endian, Notes, Section};

    trait Populate {
        fn populate(&mut self, count: usize, prime: usize) -> &mut Self;
    }

    impl Populate for Section {
        fn populate(&mut self, count: usize, prime: usize) -> &mut Self {
            for i in 0..count {
                self.append_bytes(&[((i % prime) % 256) as u8]);
            }

            self
        }
    }

    // breakpad also has a "strip self" test where it literally strips the running
    // test executable by shelling out to strip which is....yah. Can add that
    // on later but using a built-in strip via goblin or something. Or just not.

    #[template]
    #[rstest]
    #[case::class32(ElfClass::Class32)]
    #[case::class64(ElfClass::Class64)]
    fn classes(#[case] class: ElfClass) {}

    #[apply(classes)]
    fn elf_class(#[case] class: ElfClass) {
        let mut elf = synth_elf::Elf::new(elf::header::EM_386, class, Endian::Little);
        let mut text_section = Section::with_endian(Endian::Little);

        for i in 0..128u16 {
            text_section.D8((i * 3) as u8);
        }

        elf.add_section(".text", text_section, elf::section_header::SHT_PROGBITS);
        let elf_data = elf.finish().unwrap();

        let id = ElfId::from_mapped_file(&elf_data).unwrap();
        assert_eq!(id.as_uuid_string(), "80808080808000000000008080808080");
    }

    #[apply(classes)]
    fn build_id(#[case] class: ElfClass) {
        let mut elf = synth_elf::Elf::new(elf::header::EM_386, class, Endian::Little);

        // Add a text section which should _not_ be used for the build id since
        // we insert the specific build-id note
        {
            let mut text_section = Section::with_endian(Endian::Little);
            text_section.append_repeated(0, 4 * 1024);
            elf.add_section(".text", text_section, elf::section_header::SHT_PROGBITS);
        }

        let build_id = b"0123456789ABCDEFGHIJ";

        // The actual build-id we want to test for
        {
            let mut notes = Notes::with_endian(Endian::Little);
            notes.add_note(goblin::elf::note::NT_GNU_BUILD_ID, "GNU", build_id);

            elf.add_section(".note.gnu.build-id", notes, elf::section_header::SHT_NOTE);
        }

        let elf_data = elf.finish().unwrap();

        let id = ElfId::from_mapped_file(&elf_data).unwrap();
        assert_eq!(id.as_ref(), build_id);
    }

    #[apply(classes)]
    fn short_build_id(#[case] class: ElfClass) {
        let mut elf = synth_elf::Elf::new(elf::header::EM_386, class, Endian::Little);

        // Add a text section which should _not_ be used for the build id since
        // we insert the specific build-id note
        {
            let mut text_section = Section::with_endian(Endian::Little);
            text_section.append_repeated(0, 4 * 1024);
            elf.add_section(".text", text_section, elf::section_header::SHT_PROGBITS);
        }

        let build_id = b"0123";

        // The actual build-id we want to test for
        {
            let mut notes = Notes::with_endian(Endian::Little);
            notes.add_note(goblin::elf::note::NT_GNU_BUILD_ID, "GNU", build_id);

            elf.add_section(".note.gnu.build-id", notes, elf::section_header::SHT_NOTE);
        }

        let elf_data = elf.finish().unwrap();

        let id = ElfId::from_mapped_file(&elf_data).unwrap();
        assert_eq!(id.as_ref(), build_id);
    }

    #[apply(classes)]
    fn long_build_id(#[case] class: ElfClass) {
        let mut elf = synth_elf::Elf::new(elf::header::EM_386, class, Endian::Little);

        // Add a text section which should _not_ be used for the build id since
        // we insert the specific build-id note
        {
            let mut text_section = Section::with_endian(Endian::Little);
            text_section.append_repeated(0, 4 * 1024);
            elf.add_section(".text", text_section, elf::section_header::SHT_PROGBITS);
        }

        let build_id: Vec<_> = (0..32).into_iter().collect();

        // The actual build-id we want to test for
        {
            let mut notes = Notes::with_endian(Endian::Little);
            notes.add_note(goblin::elf::note::NT_GNU_BUILD_ID, "GNU", &build_id);

            elf.add_section(".note.gnu.build-id", notes, elf::section_header::SHT_NOTE);
        }

        let elf_data = elf.finish().unwrap();

        let id = ElfId::from_mapped_file(&elf_data).unwrap();
        assert_eq!(id.as_ref(), build_id);
    }

    #[apply(classes)]
    fn pt_note(#[case] class: ElfClass) {
        let mut elf = synth_elf::Elf::new(elf::header::EM_386, class, Endian::Little);

        // Add a text section which should _not_ be used for the build id since
        // we insert the specific build-id note
        {
            let mut text_section = Section::with_endian(Endian::Little);
            text_section.append_repeated(0, 4 * 1024);
            elf.add_section(".text", text_section, elf::section_header::SHT_PROGBITS);
        }

        let build_id: Vec<_> = (0..20).into_iter().collect();

        // The actual build-id we want to test for
        let index = {
            let mut notes = Notes::with_endian(Endian::Little);
            notes.add_note(0, "Linux", &[0x42, 0x2, 0, 0]);
            notes.add_note(goblin::elf::note::NT_GNU_BUILD_ID, "GNU", &build_id);

            elf.add_section(".note", notes, elf::section_header::SHT_NOTE)
        };

        elf.add_segment(index, index, elf::program_header::PT_NOTE, 0);

        let elf_data = elf.finish().unwrap();

        let id = ElfId::from_mapped_file(&elf_data).unwrap();
        assert_eq!(id.as_ref(), build_id);
    }

    #[apply(classes)]
    fn multiple_pt_notes(#[case] class: ElfClass) {
        let mut elf = synth_elf::Elf::new(elf::header::EM_386, class, Endian::Little);

        // Add a text section which should _not_ be used for the build id since
        // we insert the specific build-id note
        {
            let mut text_section = Section::with_endian(Endian::Little);
            text_section.append_repeated(0, 4 * 1024);
            elf.add_section(".text", text_section, elf::section_header::SHT_PROGBITS);
        }

        let build_id: Vec<_> = (0..20).into_iter().collect();

        {
            // Another note that we should disregard
            let mut first = Notes::with_endian(Endian::Little);
            first.add_note(0, "Linux", &[0x42, 0x2, 0, 0]);

            // The actual build-id we want to test for
            let mut second = Notes::with_endian(Endian::Little);
            second.add_note(goblin::elf::note::NT_GNU_BUILD_ID, "GNU", &build_id);

            let note1 = elf.add_section(".note1", first, elf::section_header::SHT_NOTE);
            let note2 = elf.add_section(".note2", second, elf::section_header::SHT_NOTE);

            elf.add_segment(note1, note1, elf::program_header::PT_NOTE, 0);
            elf.add_segment(note2, note2, elf::program_header::PT_NOTE, 0);
        }

        let elf_data = elf.finish().unwrap();

        let id = ElfId::from_mapped_file(&elf_data).unwrap();
        assert_eq!(id.as_ref(), build_id);
    }

    #[apply(classes)]
    fn unique_hashes(#[case] class: ElfClass) {
        let first = {
            let mut elf = synth_elf::Elf::new(elf::header::EM_386, class, Endian::Little);

            elf.add_section(
                ".foo",
                Section::inline(Some(Endian::Little), |s| s.populate(32, 5)),
                elf::section_header::SHT_PROGBITS,
            );

            elf.add_section(
                ".text",
                Section::inline(Some(Endian::Little), |s| s.populate(4 * 1024, 17)),
                elf::section_header::SHT_PROGBITS,
            );

            ElfId::from_mapped_file(&elf.finish().unwrap()).unwrap()
        };

        let second = {
            let mut elf = synth_elf::Elf::new(elf::header::EM_386, class, Endian::Little);

            elf.add_section(
                ".foo",
                Section::inline(Some(Endian::Little), |s| s.populate(32, 5)),
                elf::section_header::SHT_PROGBITS,
            );

            elf.add_section(
                ".text",
                Section::inline(Some(Endian::Little), |s| s.populate(4 * 1024, 31)),
                elf::section_header::SHT_PROGBITS,
            );

            ElfId::from_mapped_file(&elf.finish().unwrap()).unwrap()
        };

        assert_ne!(first.as_ref(), second.as_ref());
    }
}
