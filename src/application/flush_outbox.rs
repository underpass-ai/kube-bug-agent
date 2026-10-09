use crate::ports::{ObservationOutbox, ObservationSink};
use anyhow::Result;
use std::sync::Arc;
pub struct FlushOutbox {
    outbox: Arc<dyn ObservationOutbox>,
    sink: Arc<dyn ObservationSink>,
}
impl FlushOutbox {
    pub fn new(outbox: Arc<dyn ObservationOutbox>, sink: Arc<dyn ObservationSink>) -> Self {
        Self { outbox, sink }
    }
    pub async fn execute(&self) -> Result<usize> {
        let mut delivered = 0;
        for _ in 0..100 {
            let Some(observation) = self.outbox.next().await? else {
                break;
            };
            self.sink.deliver(&observation).await?;
            self.outbox.acknowledge(observation.event_id).await?;
            delivered += 1;
        }
        Ok(delivered)
    }
}
