use hashbrown::HashMap;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};
use vek::Vec2;

pub type ChunkLifecycleHandle = Arc<Mutex<ChunkLifecycleTable>>;
const RECENT_ABNORMAL_TERMINALS_LIMIT: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChunkLifecycleSource {
    ClientExplicit,
    MinVdWarmup,
    TerrainSync,
}

impl ChunkLifecycleSource {
    const fn bit(self) -> u8 {
        match self {
            Self::ClientExplicit => 1 << 0,
            Self::MinVdWarmup => 1 << 1,
            Self::TerrainSync => 1 << 2,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChunkLifecycleTerminal {
    SentOk,
    PartialSendFail,
    GenerateErr,
    Canceled,
    Dropped,
}

impl ChunkLifecycleTerminal {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SentOk => "sent_ok",
            Self::PartialSendFail => "partial_send_fail",
            Self::GenerateErr => "generate_err",
            Self::Canceled => "canceled",
            Self::Dropped => "dropped",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChunkLifecycleSourceMask(u8);

impl ChunkLifecycleSourceMask {
    fn insert(&mut self, source: ChunkLifecycleSource) { self.0 |= source.bit(); }

    pub const fn contains(self, source: ChunkLifecycleSource) -> bool { self.0 & source.bit() != 0 }

    pub const fn bits(self) -> u8 { self.0 }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChunkLifecycleEntry {
    pub source_mask: ChunkLifecycleSourceMask,
    pub requester_count: u32,
    pub first_seen_tick: u64,
    pub gen_queued_tick: Option<u64>,
    pub gen_done_tick: Option<u64>,
    pub serialize_queued_tick: Option<u64>,
    pub serialize_done_tick: Option<u64>,
    pub send_attempted_tick: Option<u64>,
    pub recipient_count: u32,
    pub terminal: Option<ChunkLifecycleTerminal>,
}

impl ChunkLifecycleEntry {
    fn new(first_seen_tick: u64) -> Self {
        Self {
            source_mask: ChunkLifecycleSourceMask::default(),
            requester_count: 0,
            first_seen_tick,
            gen_queued_tick: None,
            gen_done_tick: None,
            serialize_queued_tick: None,
            serialize_done_tick: None,
            send_attempted_tick: None,
            recipient_count: 0,
            terminal: None,
        }
    }

    fn last_observed_tick(&self) -> u64 {
        self.send_attempted_tick
            .or(self.serialize_done_tick)
            .or(self.serialize_queued_tick)
            .or(self.gen_done_tick)
            .or(self.gen_queued_tick)
            .unwrap_or(self.first_seen_tick)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ChunkLifecycleTerminalRecord {
    chunk_key: Vec2<i32>,
    entry: ChunkLifecycleEntry,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChunkLifecycleAbnormalSummary {
    recent_abnormal_count: usize,
    latest_chunk_key: Vec2<i32>,
    latest_terminal: &'static str,
    latest_tick: Option<u64>,
}

impl ChunkLifecycleAbnormalSummary {
    pub fn new(
        recent_abnormal_count: usize,
        latest_chunk_key: [i32; 2],
        latest_terminal: &'static str,
        latest_tick: Option<u64>,
    ) -> Self {
        Self {
            recent_abnormal_count,
            latest_chunk_key: Vec2::new(latest_chunk_key[0], latest_chunk_key[1]),
            latest_terminal,
            latest_tick,
        }
    }

    pub const fn recent_abnormal_count(&self) -> usize { self.recent_abnormal_count }

    pub const fn latest_chunk_key(&self) -> [i32; 2] {
        [self.latest_chunk_key.x, self.latest_chunk_key.y]
    }

    pub const fn latest_terminal_str(&self) -> &'static str { self.latest_terminal }

    pub const fn latest_tick(&self) -> Option<u64> { self.latest_tick }
}

#[derive(Default)]
pub struct ChunkLifecycleTable {
    entries: HashMap<Vec2<i32>, ChunkLifecycleEntry>,
    recent_abnormal_terminals: VecDeque<ChunkLifecycleTerminalRecord>,
}

impl ChunkLifecycleTable {
    pub fn record_source(
        &mut self,
        key: Vec2<i32>,
        source: ChunkLifecycleSource,
        tick: u64,
    ) -> &mut ChunkLifecycleEntry {
        let entry = self
            .entries
            .entry(key)
            .or_insert_with(|| ChunkLifecycleEntry::new(tick));
        entry.source_mask.insert(source);
        entry
    }

    pub fn record_request(&mut self, key: Vec2<i32>, source: ChunkLifecycleSource, tick: u64) {
        let entry = self.record_source(key, source, tick);
        entry.requester_count = entry.requester_count.saturating_add(1);
    }

    pub fn record_generation_queued(&mut self, key: Vec2<i32>, tick: u64) {
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.gen_queued_tick.get_or_insert(tick);
        }
    }

    pub fn record_generation_done(&mut self, key: Vec2<i32>, tick: u64) {
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.gen_done_tick = Some(tick);
        }
    }

    pub fn record_serialize_queued(&mut self, key: Vec2<i32>, tick: u64, recipient_count: usize) {
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.serialize_queued_tick = Some(tick);
            entry.recipient_count = entry.recipient_count.max(recipient_count as u32);
        }
    }

    pub fn record_serialize_done(&mut self, key: Vec2<i32>, tick: u64) {
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.serialize_done_tick = Some(tick);
        }
    }

    pub fn complete(
        &mut self,
        key: Vec2<i32>,
        tick: Option<u64>,
        terminal: ChunkLifecycleTerminal,
        recipient_count: Option<usize>,
    ) -> Option<ChunkLifecycleEntry> {
        let mut entry = self.entries.remove(&key)?;
        entry.send_attempted_tick = tick;
        if let Some(recipient_count) = recipient_count {
            entry.recipient_count = entry.recipient_count.max(recipient_count as u32);
        }
        entry.terminal = Some(terminal);
        let records_abnormal_evidence = matches!(
            terminal,
            ChunkLifecycleTerminal::PartialSendFail | ChunkLifecycleTerminal::GenerateErr
        ) || matches!(terminal, ChunkLifecycleTerminal::Dropped if tick.is_none());
        if records_abnormal_evidence {
            self.record_abnormal_terminal(key, &entry);
        }
        Some(entry)
    }

    pub fn active_entries_len(&self) -> usize { self.entries.len() }

    pub fn abnormal_summary(&self) -> Option<ChunkLifecycleAbnormalSummary> {
        let latest = self.recent_abnormal_terminals.front()?;
        Some(ChunkLifecycleAbnormalSummary::new(
            self.recent_abnormal_terminals.len(),
            [latest.chunk_key.x, latest.chunk_key.y],
            latest
                .entry
                .terminal
                .expect("abnormal terminal record should always include terminal")
                .as_str(),
            latest.entry.send_attempted_tick,
        ))
    }

    pub fn prune_stale(&mut self, oldest_allowed_tick: u64) {
        self.entries
            .retain(|_, entry| entry.last_observed_tick() >= oldest_allowed_tick);
    }

    fn record_abnormal_terminal(&mut self, key: Vec2<i32>, entry: &ChunkLifecycleEntry) {
        self.recent_abnormal_terminals
            .push_front(ChunkLifecycleTerminalRecord {
                chunk_key: key,
                entry: entry.clone(),
            });
        if self.recent_abnormal_terminals.len() > RECENT_ABNORMAL_TERMINALS_LIMIT {
            self.recent_abnormal_terminals
                .truncate(RECENT_ABNORMAL_TERMINALS_LIMIT);
        }
    }

    #[cfg(test)]
    pub fn entry(&self, key: Vec2<i32>) -> Option<&ChunkLifecycleEntry> { self.entries.get(&key) }
}

pub fn new_chunk_lifecycle_handle() -> ChunkLifecycleHandle {
    Arc::new(Mutex::new(ChunkLifecycleTable::default()))
}

#[cfg(test)]
mod tests {
    use super::{ChunkLifecycleSource, ChunkLifecycleTable, ChunkLifecycleTerminal};
    use vek::Vec2;

    #[test]
    fn request_tracking_aggregates_sources_and_requesters() {
        let key = Vec2::new(3, 7);
        let mut table = ChunkLifecycleTable::default();

        table.record_request(key, ChunkLifecycleSource::ClientExplicit, 10);
        table.record_request(key, ChunkLifecycleSource::MinVdWarmup, 11);
        table.record_source(key, ChunkLifecycleSource::TerrainSync, 12);

        let entry = table.entry(key).expect("entry should exist");
        assert_eq!(entry.first_seen_tick, 10);
        assert_eq!(entry.requester_count, 2);
        assert!(
            entry
                .source_mask
                .contains(ChunkLifecycleSource::ClientExplicit)
        );
        assert!(
            entry
                .source_mask
                .contains(ChunkLifecycleSource::MinVdWarmup)
        );
        assert!(
            entry
                .source_mask
                .contains(ChunkLifecycleSource::TerrainSync)
        );
    }

    #[test]
    fn completion_removes_entry_and_preserves_terminal_snapshot() {
        let key = Vec2::new(5, 9);
        let mut table = ChunkLifecycleTable::default();
        table.record_request(key, ChunkLifecycleSource::ClientExplicit, 20);
        table.record_generation_queued(key, 21);
        table.record_generation_done(key, 22);
        table.record_serialize_queued(key, 23, 4);
        table.record_serialize_done(key, 24);

        let completed = table
            .complete(
                key,
                Some(25),
                ChunkLifecycleTerminal::PartialSendFail,
                Some(3),
            )
            .expect("entry should complete");

        assert_eq!(
            completed.terminal,
            Some(ChunkLifecycleTerminal::PartialSendFail)
        );
        assert_eq!(completed.send_attempted_tick, Some(25));
        assert_eq!(completed.recipient_count, 4);
        assert!(table.entry(key).is_none());
    }

    #[test]
    fn dropped_without_send_tick_enters_abnormal_buffer() {
        let key = Vec2::new(8, 13);
        let mut table = ChunkLifecycleTable::default();
        table.record_request(key, ChunkLifecycleSource::ClientExplicit, 30);
        table.record_generation_queued(key, 31);
        table.record_generation_done(key, 32);
        table.record_serialize_queued(key, 33, 2);

        let _ = table.complete(key, None, ChunkLifecycleTerminal::Dropped, Some(2));

        let summary = table
            .abnormal_summary()
            .expect("dropped terminal should produce abnormal summary");
        assert_eq!(summary.recent_abnormal_count(), 1);
        assert_eq!(summary.latest_chunk_key(), [8, 13]);
        assert_eq!(summary.latest_terminal_str(), "dropped");
        assert_eq!(summary.latest_tick(), None);
    }

    #[test]
    fn dropped_with_send_tick_does_not_enter_abnormal_buffer() {
        let key = Vec2::new(9, 14);
        let mut table = ChunkLifecycleTable::default();
        table.record_request(key, ChunkLifecycleSource::ClientExplicit, 30);
        table.record_generation_queued(key, 31);
        table.record_generation_done(key, 32);
        table.record_serialize_queued(key, 33, 2);

        let _ = table.complete(key, Some(34), ChunkLifecycleTerminal::Dropped, Some(2));

        assert!(table.abnormal_summary().is_none());
    }

    #[test]
    fn sent_ok_does_not_enter_abnormal_buffer() {
        let key = Vec2::new(21, 34);
        let mut table = ChunkLifecycleTable::default();
        table.record_request(key, ChunkLifecycleSource::ClientExplicit, 40);

        let _ = table.complete(key, Some(41), ChunkLifecycleTerminal::SentOk, Some(1));

        assert!(table.abnormal_summary().is_none());
    }
}
