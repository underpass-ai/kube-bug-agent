#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionReason {
    Baseline,
    CounterReset,
    InsufficientTraffic,
    Cooldown,
    Hold,
    Reduce,
    Recover,
    AtMinimum,
    AtMaximum,
}
