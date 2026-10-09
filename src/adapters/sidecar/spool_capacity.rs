use anyhow::{Result, ensure};
#[derive(Clone, Copy)]
pub struct SpoolCapacity(u32);
impl SpoolCapacity {
    pub fn new(value: u32) -> Result<Self> {
        ensure!(
            (1..=10_000).contains(&value),
            "spool capacity must be 1..10000"
        );
        Ok(Self(value))
    }
    pub fn value(self) -> u32 {
        self.0
    }
}
