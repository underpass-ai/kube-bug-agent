#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct RequestCount(u64);

impl RequestCount {
    pub fn new(value: u64) -> Self {
        Self(value)
    }
    pub fn value(self) -> u64 {
        self.0
    }
}
