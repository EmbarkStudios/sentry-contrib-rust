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
