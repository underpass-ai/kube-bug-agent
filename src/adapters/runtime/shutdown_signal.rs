use tokio::signal::unix::{Signal, SignalKind, signal};

pub struct ShutdownSignal {
    terminate: Signal,
    interrupt: Signal,
}
impl ShutdownSignal {
    pub fn new() -> std::io::Result<Self> {
        Ok(Self {
            terminate: signal(SignalKind::terminate())?,
            interrupt: signal(SignalKind::interrupt())?,
        })
    }

    pub async fn wait(&mut self) {
        tokio::select! {
            _ = self.terminate.recv() => {},
            _ = self.interrupt.recv() => {},
        }
    }
}
