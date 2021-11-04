cfg_if::cfg_if! {
    if #[cfg(target_pointer_width = "64")] {
        use goblin::elf64 as elf;
    } else if #[cfg(target_pointer_width = "32")] {
        use goblin::elf32 as elf;
    } else {
        compile_error!("unsupported pointer size");
    }
}

use elf::{header::Header, program_header::ProgramHeader, section_header::SectionHeader};

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

impl ElfId {
    fn new(slice: &[u8]) -> Option<Self> {
        (slice.len() <= MAX_ID_SIZE).then(|| {
            let id = [0u8; MAX_ID_SIZE];

            id[..slice.len()].copy_from_slice(slice);

            Self {
                id,
                len: slice.len(),
            }
        })
    }

    pub fn from_mapped_file(elf: &[u8]) -> Option<Self> {
        // Unfortunately, for ease of use, the batteries included elf parser in
        // goblin performs heap allocations, so we need to fall back to lazy
        // parsing ourselves
        let mut header_bytes = [0u8; elf::header::SIZEOF_EHDR];

        if elf.len() < elf::header::SIZEOF_EHDR {
            return None;
        }

        header_bytes.copy_from_slice(&elf[..elf::header::SIZEOF_EHDR]);
        let header = Header::from_bytes(&header_bytes);

        // Attempt to lookup the build-id embedded by the linker, but if no
        // build id is found, fallback to hashing the .text section
        read_build_id_note(header, elf).or_else(|| hash_text_section(header, elf))
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
            let mut align = || {
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
            align();

            let description = this.gread_with::<&'buffer [u8]>(offset, desc_size as usize)?;
            align();

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

fn find_section_by_name<'buffer>(
    header: &Header,
    elf: &'buffer [u8],
    name: &str,
    kind: u32,
) -> Option<&'buffer [u8]> {
    if header.e_shoff == 0 {
        return None;
    }

    let section_headers: &[SectionHeader] = unsafe {
        std::mem::transmute(
            &elf[header.e_shoff as usize
                ..header.e_shoff as usize
                    + std::mem::size_of::<SectionHeader>() * header.e_shnum as usize],
        )
    };

    let names_section = &section_headers[header.e_shstrndx as usize];
    let names = &elf[names_section.sh_offset as usize
        ..names_section.sh_offset as usize + names_section.sh_size as usize];

    let name = name.as_bytes();

    for sh in section_headers {
        let name_end = sh.sh_name as usize + name.len();
        if name_end > names.len() {
            continue;
        }

        let section_name = &names[sh.sh_name as usize..name_end];
        if sh.sh_type == kind && name == section_name {
            return Some(&elf[sh.sh_offset as usize..sh.sh_offset as usize + sh.sh_size as usize]);
        }
    }

    None
}

fn iter_segments<'buffer>(
    header: &Header,
    elf: &'buffer [u8],
    kind: u32,
) -> impl Iterator<Item = &'buffer [u8]> {
    let program_headers: &[ProgramHeader] = unsafe {
        std::mem::transmute(
            &elf[header.e_phoff as usize
                ..header.e_shoff as usize
                    + std::mem::size_of::<ProgramHeader>() * header.e_phnum as usize],
        )
    };

    program_headers.iter().filter_map(move |ph| {
        (ph.p_type == kind)
            .then(|| &elf[ph.p_offset as usize..ph.p_offset as usize + ph.p_filesz as usize])
    })
}

fn read_build_id_note(header: &Header, elf: &[u8]) -> Option<ElfId> {
    // lld normally creates 2 PT_NOTEs, ld/gold normally creates 1.
    for note in iter_segments(header, elf, goblin::elf::program_header::PT_NOTE) {
        if let Some(elf_id) = build_id_from_note(note) {
            return Some(elf_id);
        }
    }

    let build_id_section = find_section_by_name(
        header,
        elf,
        ".note.gnu.build-id",
        goblin::elf::section_header::SHT_NOTE,
    )?;
    build_id_from_note(build_id_section)
}

fn hash_text_section(header: &Header, elf: &[u8]) -> Option<ElfId> {
    let text_section = find_section_by_name(
        header,
        elf,
        ".text",
        goblin::elf::section_header::SHT_PROGBITS,
    )?;

    // Breakpad limits this to 16-bytes (GUID-ish) size for backwards compat, so
    // we do the same, not that this method should really ever be used in practice
    // since stripping out build ids is not a good idea
    let identifier = [0u8; 16];

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
