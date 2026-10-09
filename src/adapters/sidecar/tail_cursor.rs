#[derive(Default, serde::Deserialize, serde::Serialize)]
pub(super) struct TailCursor {
    pub inode: u64,
    pub device: u64,
    pub offset: u64,
    pub generation: u64,
    pub skipping: bool,
}
