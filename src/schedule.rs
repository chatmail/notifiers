use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::path::Path;
use std::sync::Mutex;
use std::time::SystemTime;

use anyhow::Result;

#[derive(Debug)]
pub struct Schedule {
    /// Database to persist tokens and latest notification time.
    db: sled::Db,

    /// Min-heap of tokens prioritized by the latest notification timestamp.
    heap: Mutex<BinaryHeap<(Reverse<u64>, String)>>,
}

impl Schedule {
    pub fn new(db_path: &Path) -> Result<Self> {
        let db = sled::open(db_path)?;
        let mut heap = BinaryHeap::new();
        for entry in db.iter() {
            let (key, value) = entry?;
            let token = String::from_utf8(key.to_vec()).unwrap();

            let timestamp = if let Some(value) = value.get(..8) {
                let mut buf: [u8; 8] = [0; 8];
                buf.copy_from_slice(&value[..8]);
                u64::from_be_bytes(buf)
            } else {
                0
            };
            heap.push((Reverse(timestamp), token))
        }
        let heap = Mutex::new(heap);
        Ok(Self { db, heap })
    }

    /// Registers a new heartbeat notification token.
    ///
    /// This should also be called after successful notification
    /// to update latest notification time.
    pub fn insert_token(&self, token: &str, now: u64) -> Result<()> {
        self.db.insert(token.as_bytes(), &u64::to_be_bytes(now))?;
        let mut heap = self.heap.lock().unwrap();
        heap.push((Reverse(now), token.to_owned()));
        Ok(())
    }

    pub fn insert_token_now(&self, token: &str) -> Result<()> {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.insert_token(token, now)
    }

    pub async fn flush(&self) -> Result<()> {
        self.db.flush_async().await?;
        Ok(())
    }

    /// Removes token from the schedule.
    pub fn remove_token(&self, token: &str) -> Result<()> {
        self.db.remove(token)?;
        Ok(())
    }

    pub fn pop(&self) -> Option<(u64, String)> {
        let mut heap = self.heap.lock().unwrap();
        let (timestamp, token) = heap.pop()?;
        Some((timestamp.0, token))
    }

    /// Returns the number of tokens in the schedule.
    pub fn token_count(&self) -> usize {
        let heap = self.heap.lock().unwrap();
        heap.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::tempdir;

    #[async_std::test]
    async fn test_schedule() -> Result<()> {
        let dir = tempdir()?;
        let db_path = dir.path().join("db.sled");
        let schedule = Schedule::new(&db_path)?;

        schedule.insert_token("foo", 10)?;
        schedule.insert_token("bar", 20)?;

        let (first_timestamp, first_token) = schedule.pop().unwrap();
        assert_eq!(first_timestamp, 10);
        assert_eq!(first_token, "foo");
        schedule.insert_token("foo", 30)?;
        schedule.flush().await?;

        // Reopen to test persistence.
        drop(schedule);
        let schedule = Schedule::new(&db_path)?;

        let (second_timestamp, second_token) = schedule.pop().unwrap();
        assert_eq!(second_timestamp, 20);
        assert_eq!(second_token, "bar");

        // Simulate restart or crash, token "bar" was not reinserted or removed by the app.
        drop(schedule);
        let schedule = Schedule::new(&db_path)?;

        // Token "bar" is still there.
        let (second_timestamp, second_token) = schedule.pop().unwrap();
        assert_eq!(second_timestamp, 20);
        assert_eq!(second_token, "bar");

        Ok(())
    }
}
