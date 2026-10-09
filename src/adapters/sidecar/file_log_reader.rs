use super::{JsonLogDecoder, SidecarConfig, SqliteOutbox};
use crate::{application::LogDetector, domain::Observation};
use anyhow::{Context, Result};
use std::os::unix::fs::MetadataExt;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncSeekExt, BufReader};

pub struct FileLogReader;
impl FileLogReader {
    pub async fn read(
        config: &SidecarConfig,
        outbox: &SqliteOutbox,
        require_file: bool,
    ) -> Result<u32> {
        let file = match tokio::fs::File::open(&config.log).await {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && !require_file => {
                return Ok(0);
            }
            Err(error) => return Err(error).context("cannot open application log"),
        };
        let metadata = file.metadata().await?;
        let path = config.log.to_string_lossy();
        let mut cursor = outbox.cursor(&path)?;
        if cursor.inode != metadata.ino()
            || cursor.device != metadata.dev()
            || cursor.offset > metadata.len()
        {
            cursor.generation += 1;
            cursor.offset = 0;
            cursor.skipping = false;
        }
        cursor.inode = metadata.ino();
        cursor.device = metadata.dev();
        let mut file = file;
        file.seek(std::io::SeekFrom::Start(cursor.offset)).await?;
        let mut reader = BufReader::new(file);
        let mut consumed = 0;
        for _ in 0..100 {
            let mut line = Vec::new();
            let count = (&mut reader)
                .take(65_537)
                .read_until(b'\n', &mut line)
                .await?;
            if count == 0 {
                break;
            }
            let complete = line.last() == Some(&b'\n');
            if !complete && count < 65_537 && !cursor.skipping {
                break;
            }
            let start = cursor.offset;
            let observation = if complete && !cursor.skipping && count <= 32_768 {
                let id = Observation::source_event_id(&serde_json::to_vec(&(
                    &config.workload.pod_uid,
                    cursor.device,
                    cursor.inode,
                    cursor.generation,
                    start,
                    &line,
                ))?);
                JsonLogDecoder::decode(&String::from_utf8_lossy(&line))
                    .and_then(|log| LogDetector::detect(log, id, &config.workload))
            } else {
                None
            };
            cursor.offset += count as u64;
            cursor.skipping = !complete;
            if !outbox.commit_line(&path, &cursor, observation)? {
                break;
            }
            consumed += 1;
        }
        Ok(consumed)
    }
}
