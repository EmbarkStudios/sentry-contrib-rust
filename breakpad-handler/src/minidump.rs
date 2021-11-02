pub enum MinidumpOutput {
    Path(std::path::PathBuf),
}

impl MinidumpOutput {
    #[inline]
    pub fn with_path(base: &impl AsRef<std::path::Path>) -> Self {
        Self::with_uuid(base, uuid::Uuid::new_v4())
    }

    #[inline]
    pub fn with_uuid(base: &impl AsRef<std::path::Path>, uuid: uuid::Uuid) -> Self {
        let mut pb = base.as_ref().join(uuid.to_simple().to_string());
        pb.set_extension("dmp");
        Self::Path(pb)
    }
}

pub(crate) use minidump_common::format::{
    self, MINIDUMP_DIRECTORY as Directory, MINIDUMP_HEADER as Header,
    MINIDUMP_LOCATION_DESCRIPTOR as Location, MINIDUMP_MEMORY_DESCRIPTOR as MemoryDescriptor,
    MINIDUMP_STREAM_TYPE as StreamType, MINIDUMP_THREAD as Thread,
};
