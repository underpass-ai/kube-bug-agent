use std::path::PathBuf;
pub struct TestWorkspace {
    root: PathBuf,
}
impl TestWorkspace {
    pub fn new() -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tmp")
            .join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&root).unwrap();
        Self { root }
    }
    pub fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }
}
impl Drop for TestWorkspace {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}
