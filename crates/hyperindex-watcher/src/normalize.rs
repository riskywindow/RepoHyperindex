use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use hyperindex_protocol::watch::{NormalizedEvent, NormalizedEventKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawWatchEventKind {
    Created,
    Modified,
    Removed,
    Renamed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawWatchEvent {
    pub kind: RawWatchEventKind,
    pub path: String,
    pub previous_path: Option<String>,
}

#[derive(Debug)]
struct PendingEvent {
    event: PendingNormalizedEvent,
    last_update: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingNormalizedEvent {
    kind: NormalizedEventKind,
    path: String,
    previous_path: Option<String>,
}

#[derive(Debug)]
pub struct WatchEventStream {
    debounce_window: Duration,
    batch_max_events: usize,
    next_sequence: u64,
    dropped_events: u64,
    pending: BTreeMap<String, PendingEvent>,
}

impl WatchEventStream {
    pub fn new(debounce_ms: u64, batch_max_events: usize) -> Self {
        Self {
            debounce_window: Duration::from_millis(debounce_ms),
            batch_max_events,
            next_sequence: 1,
            dropped_events: 0,
            pending: BTreeMap::new(),
        }
    }

    pub fn push_raw_batch(&mut self, events: Vec<RawWatchEvent>, now: Instant) {
        for event in events {
            self.push_raw_event(event, now);
        }
    }

    pub fn drain_ready(&mut self, now: Instant) -> Vec<NormalizedEvent> {
        let ready_keys = self
            .pending
            .iter()
            .filter_map(|(key, pending)| {
                (now.duration_since(pending.last_update) >= self.debounce_window)
                    .then(|| key.clone())
            })
            .collect::<Vec<_>>();
        self.flush_keys(ready_keys)
    }

    pub fn flush_all(&mut self) -> Vec<NormalizedEvent> {
        let keys = self.pending.keys().cloned().collect::<Vec<_>>();
        self.flush_keys(keys)
    }

    pub fn dropped_events(&self) -> u64 {
        self.dropped_events
    }

    fn push_raw_event(&mut self, event: RawWatchEvent, now: Instant) {
        let mut normalized = PendingNormalizedEvent {
            kind: raw_kind_to_normalized(&event.kind),
            path: event.path,
            previous_path: event.previous_path,
        };

        if normalized.kind == NormalizedEventKind::Renamed {
            if let Some(previous_path) = normalized.previous_path.clone() {
                if let Some(previous) = self.pending.remove(&previous_path) {
                    normalized = merge_rename(previous.event, normalized);
                }
            }
        }

        let key = normalized.path.clone();
        if let Some(existing) = self.pending.remove(&key) {
            match merge_events(existing.event, normalized) {
                Some(merged) => self.insert_pending(merged, now),
                None => {}
            }
        } else {
            self.insert_pending(normalized, now);
        }
    }

    fn insert_pending(&mut self, event: PendingNormalizedEvent, now: Instant) {
        if self.pending.len() >= self.batch_max_events && !self.pending.contains_key(&event.path) {
            self.dropped_events += 1;
            return;
        }

        self.pending.insert(
            event.path.clone(),
            PendingEvent {
                event,
                last_update: now,
            },
        );
    }

    fn flush_keys(&mut self, mut keys: Vec<String>) -> Vec<NormalizedEvent> {
        keys.sort();
        let mut flushed = Vec::new();
        for key in keys {
            if let Some(pending) = self.pending.remove(&key) {
                flushed.push(pending.event);
            }
        }
        flushed.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then_with(|| left.previous_path.cmp(&right.previous_path))
                .then_with(|| kind_rank(&left.kind).cmp(&kind_rank(&right.kind)))
        });
        flushed
            .into_iter()
            .map(|event| {
                let sequence = self.next_sequence;
                self.next_sequence += 1;
                NormalizedEvent {
                    sequence,
                    kind: event.kind,
                    path: event.path,
                    previous_path: event.previous_path,
                }
            })
            .collect()
    }
}

fn raw_kind_to_normalized(kind: &RawWatchEventKind) -> NormalizedEventKind {
    match kind {
        RawWatchEventKind::Created => NormalizedEventKind::Created,
        RawWatchEventKind::Modified => NormalizedEventKind::Modified,
        RawWatchEventKind::Removed => NormalizedEventKind::Removed,
        RawWatchEventKind::Renamed => NormalizedEventKind::Renamed,
    }
}

fn merge_rename(
    existing: PendingNormalizedEvent,
    renamed: PendingNormalizedEvent,
) -> PendingNormalizedEvent {
    match existing.kind {
        NormalizedEventKind::Created => PendingNormalizedEvent {
            kind: NormalizedEventKind::Created,
            path: renamed.path,
            previous_path: None,
        },
        NormalizedEventKind::Renamed => PendingNormalizedEvent {
            kind: NormalizedEventKind::Renamed,
            path: renamed.path,
            previous_path: existing.previous_path.or(renamed.previous_path),
        },
        _ => PendingNormalizedEvent {
            kind: NormalizedEventKind::Renamed,
            path: renamed.path,
            previous_path: Some(existing.path),
        },
    }
}

fn merge_events(
    existing: PendingNormalizedEvent,
    incoming: PendingNormalizedEvent,
) -> Option<PendingNormalizedEvent> {
    match (&existing.kind, &incoming.kind) {
        (NormalizedEventKind::Created, NormalizedEventKind::Modified) => Some(existing),
        (NormalizedEventKind::Created, NormalizedEventKind::Removed) => None,
        (NormalizedEventKind::Modified, NormalizedEventKind::Modified) => Some(existing),
        (NormalizedEventKind::Modified, NormalizedEventKind::Removed) => Some(incoming),
        (NormalizedEventKind::Removed, NormalizedEventKind::Created) => {
            Some(PendingNormalizedEvent {
                kind: NormalizedEventKind::Modified,
                path: incoming.path,
                previous_path: None,
            })
        }
        (NormalizedEventKind::Removed, NormalizedEventKind::Modified) => {
            Some(PendingNormalizedEvent {
                kind: NormalizedEventKind::Modified,
                path: incoming.path,
                previous_path: None,
            })
        }
        (NormalizedEventKind::Renamed, NormalizedEventKind::Modified) => Some(existing),
        (NormalizedEventKind::Renamed, NormalizedEventKind::Removed) => Some(incoming),
        _ => Some(incoming),
    }
}

fn kind_rank(kind: &NormalizedEventKind) -> u8 {
    match kind {
        NormalizedEventKind::Created => 0,
        NormalizedEventKind::Modified => 1,
        NormalizedEventKind::Removed => 2,
        NormalizedEventKind::Renamed => 3,
        NormalizedEventKind::Other => 4,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{RawWatchEvent, RawWatchEventKind, WatchEventStream};
    use hyperindex_protocol::watch::NormalizedEventKind;

    #[test]
    fn stream_coalesces_burst_modifications() {
        let mut stream = WatchEventStream::new(50, 32);
        let now = std::time::Instant::now();
        stream.push_raw_batch(
            vec![
                RawWatchEvent {
                    kind: RawWatchEventKind::Modified,
                    path: "src/app.ts".to_string(),
                    previous_path: None,
                },
                RawWatchEvent {
                    kind: RawWatchEventKind::Modified,
                    path: "src/app.ts".to_string(),
                    previous_path: None,
                },
            ],
            now,
        );

        let events = stream.drain_ready(now + Duration::from_millis(60));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, NormalizedEventKind::Modified);
        assert_eq!(events[0].path, "src/app.ts");
    }

    #[test]
    fn stream_coalesces_create_then_remove() {
        let mut stream = WatchEventStream::new(50, 32);
        let now = std::time::Instant::now();
        stream.push_raw_batch(
            vec![
                RawWatchEvent {
                    kind: RawWatchEventKind::Created,
                    path: "tmp/file.txt".to_string(),
                    previous_path: None,
                },
                RawWatchEvent {
                    kind: RawWatchEventKind::Removed,
                    path: "tmp/file.txt".to_string(),
                    previous_path: None,
                },
            ],
            now,
        );

        let events = stream.flush_all();
        assert!(events.is_empty());
    }
}
