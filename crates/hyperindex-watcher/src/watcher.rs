use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use hyperindex_core::{HyperindexError, HyperindexResult};
use hyperindex_protocol::config::{WatchBackend as ConfigWatchBackend, WatchConfig};
use hyperindex_protocol::watch::NormalizedEvent;

use crate::ignore::IgnoreMatcher;
use crate::normalize::{RawWatchEvent, RawWatchEventKind, WatchEventStream};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct FileFingerprint {
    size: u64,
    content_hash: u64,
}

pub trait WatchBackend {
    fn backend_name(&self) -> &'static str;
    fn poll(&mut self) -> HyperindexResult<Vec<RawWatchEvent>>;
}

#[derive(Debug)]
pub struct PollingWatcher {
    repo_root: PathBuf,
    ignore_matcher: IgnoreMatcher,
    previous_snapshot: BTreeMap<String, FileFingerprint>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchRun {
    pub backend: String,
    pub dropped_events: u64,
    pub events: Vec<NormalizedEvent>,
}

#[derive(Debug)]
pub struct WatcherService<B: WatchBackend> {
    backend: B,
    stream: WatchEventStream,
    poll_interval: Duration,
}

impl PollingWatcher {
    pub fn new(
        repo_root: impl Into<PathBuf>,
        ignore_patterns: Vec<String>,
    ) -> HyperindexResult<Self> {
        let repo_root = canonicalize_dir(repo_root.into())?;
        let ignore_matcher = IgnoreMatcher::new(ignore_patterns);
        let previous_snapshot = collect_snapshot(&repo_root, &ignore_matcher)?;
        Ok(Self {
            repo_root,
            ignore_matcher,
            previous_snapshot,
        })
    }
}

impl WatchBackend for PollingWatcher {
    fn backend_name(&self) -> &'static str {
        "poll"
    }

    fn poll(&mut self) -> HyperindexResult<Vec<RawWatchEvent>> {
        let current_snapshot = collect_snapshot(&self.repo_root, &self.ignore_matcher)?;
        let events = diff_snapshots(&self.previous_snapshot, &current_snapshot);
        self.previous_snapshot = current_snapshot;
        Ok(events)
    }
}

impl WatcherService<PollingWatcher> {
    pub fn polling(
        repo_root: impl Into<PathBuf>,
        config: WatchConfig,
        ignore_patterns: Vec<String>,
    ) -> HyperindexResult<Self> {
        let backend = match config.backend {
            ConfigWatchBackend::Poll | ConfigWatchBackend::Stub => {
                PollingWatcher::new(repo_root, ignore_patterns)?
            }
            ConfigWatchBackend::Notify => {
                return Err(HyperindexError::NotImplemented("watcher.notify_backend"));
            }
        };
        Ok(Self {
            backend,
            stream: WatchEventStream::new(config.debounce_ms, config.batch_max_events),
            poll_interval: Duration::from_millis(config.poll_interval_ms.max(10)),
        })
    }
}

impl<B: WatchBackend> WatcherService<B> {
    pub fn watch_once(&mut self, timeout: Duration) -> HyperindexResult<WatchRun> {
        let deadline = Instant::now() + timeout;
        let mut events = Vec::new();

        loop {
            let now = Instant::now();
            let raw_events = self.backend.poll()?;
            self.stream.push_raw_batch(raw_events, now);
            events.extend(self.stream.drain_ready(now));

            if now >= deadline {
                break;
            }

            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            thread::sleep(self.poll_interval.min(remaining));
        }

        events.extend(self.stream.flush_all());
        Ok(WatchRun {
            backend: self.backend.backend_name().to_string(),
            dropped_events: self.stream.dropped_events(),
            events,
        })
    }
}

fn canonicalize_dir(path: PathBuf) -> HyperindexResult<PathBuf> {
    let absolute = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .map_err(|error| HyperindexError::Message(format!("current_dir failed: {error}")))?
            .join(path)
    };
    let canonical = fs::canonicalize(&absolute).map_err(|error| {
        HyperindexError::Message(format!(
            "failed to canonicalize {}: {error}",
            absolute.display()
        ))
    })?;
    if !canonical.is_dir() {
        return Err(HyperindexError::Message(format!(
            "{} is not a directory",
            canonical.display()
        )));
    }
    Ok(canonical)
}

fn collect_snapshot(
    repo_root: &Path,
    ignore_matcher: &IgnoreMatcher,
) -> HyperindexResult<BTreeMap<String, FileFingerprint>> {
    let mut snapshot = BTreeMap::new();
    visit_dir(repo_root, repo_root, ignore_matcher, &mut snapshot)?;
    Ok(snapshot)
}

fn visit_dir(
    repo_root: &Path,
    current_dir: &Path,
    ignore_matcher: &IgnoreMatcher,
    snapshot: &mut BTreeMap<String, FileFingerprint>,
) -> HyperindexResult<()> {
    let mut entries = fs::read_dir(current_dir)
        .map_err(|error| HyperindexError::Message(format!("read_dir failed: {error}")))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| HyperindexError::Message(format!("dir entry failed: {error}")))?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let path = entry.path();
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(HyperindexError::Message(format!(
                    "metadata failed for {}: {error}",
                    path.display()
                )));
            }
        };

        let relative_path = relative_path(repo_root, &path)?;
        if metadata.is_dir() {
            if ignore_matcher.should_skip_dir(&relative_path) {
                continue;
            }
            visit_dir(repo_root, &path, ignore_matcher, snapshot)?;
        } else if metadata.is_file() {
            if ignore_matcher.is_ignored(&relative_path) {
                continue;
            }
            if let Some(fingerprint) = fingerprint_file(&path)? {
                snapshot.insert(relative_path, fingerprint);
            }
        }
    }

    Ok(())
}

fn fingerprint_file(path: &Path) -> HyperindexResult<Option<FileFingerprint>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(HyperindexError::Message(format!(
                "failed to read {}: {error}",
                path.display()
            )));
        }
    };

    Ok(Some(FileFingerprint {
        size: bytes.len() as u64,
        content_hash: stable_hash(&bytes),
    }))
}

fn diff_snapshots(
    previous: &BTreeMap<String, FileFingerprint>,
    current: &BTreeMap<String, FileFingerprint>,
) -> Vec<RawWatchEvent> {
    let mut created = Vec::new();
    let mut removed = Vec::new();
    let mut modified = Vec::new();

    for (path, fingerprint) in current {
        match previous.get(path) {
            Some(existing) if existing != fingerprint => modified.push(path.clone()),
            None => created.push((path.clone(), fingerprint.clone())),
            _ => {}
        }
    }

    for (path, fingerprint) in previous {
        if !current.contains_key(path) {
            removed.push((path.clone(), fingerprint.clone()));
        }
    }

    created.sort_by(|left, right| left.0.cmp(&right.0));
    removed.sort_by(|left, right| left.0.cmp(&right.0));
    modified.sort();

    let mut created_by_fingerprint = BTreeMap::<FileFingerprint, Vec<String>>::new();
    for (path, fingerprint) in &created {
        created_by_fingerprint
            .entry(fingerprint.clone())
            .or_default()
            .push(path.clone());
    }

    let mut removed_by_fingerprint = BTreeMap::<FileFingerprint, Vec<String>>::new();
    for (path, fingerprint) in &removed {
        removed_by_fingerprint
            .entry(fingerprint.clone())
            .or_default()
            .push(path.clone());
    }

    let mut renamed = Vec::new();
    let mut matched_created = std::collections::BTreeSet::new();
    let mut matched_removed = std::collections::BTreeSet::new();

    for (fingerprint, created_paths) in &created_by_fingerprint {
        if created_paths.len() == 1 {
            if let Some(removed_paths) = removed_by_fingerprint.get(fingerprint) {
                if removed_paths.len() == 1 {
                    let new_path = created_paths[0].clone();
                    let old_path = removed_paths[0].clone();
                    matched_created.insert(new_path.clone());
                    matched_removed.insert(old_path.clone());
                    renamed.push(RawWatchEvent {
                        kind: RawWatchEventKind::Renamed,
                        path: new_path,
                        previous_path: Some(old_path),
                    });
                }
            }
        }
    }

    let mut events = Vec::new();
    events.extend(renamed);
    events.extend(created.into_iter().filter_map(|(path, _)| {
        (!matched_created.contains(&path)).then_some(RawWatchEvent {
            kind: RawWatchEventKind::Created,
            path,
            previous_path: None,
        })
    }));
    events.extend(modified.into_iter().map(|path| RawWatchEvent {
        kind: RawWatchEventKind::Modified,
        path,
        previous_path: None,
    }));
    events.extend(removed.into_iter().filter_map(|(path, _)| {
        (!matched_removed.contains(&path)).then_some(RawWatchEvent {
            kind: RawWatchEventKind::Removed,
            path,
            previous_path: None,
        })
    }));
    events.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.previous_path.cmp(&right.previous_path))
            .then_with(|| raw_kind_rank(&left.kind).cmp(&raw_kind_rank(&right.kind)))
    });
    events
}

fn relative_path(repo_root: &Path, path: &Path) -> HyperindexResult<String> {
    let relative = path.strip_prefix(repo_root).map_err(|error| {
        HyperindexError::Message(format!(
            "failed to strip repo root {} from {}: {error}",
            repo_root.display(),
            path.display()
        ))
    })?;
    relative
        .to_str()
        .map(|value| value.replace('\\', "/"))
        .ok_or_else(|| HyperindexError::Message(format!("non-utf8 path: {}", path.display())))
}

fn stable_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn raw_kind_rank(kind: &RawWatchEventKind) -> u8 {
    match kind {
        RawWatchEventKind::Created => 0,
        RawWatchEventKind::Modified => 1,
        RawWatchEventKind::Removed => 2,
        RawWatchEventKind::Renamed => 3,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::thread;
    use std::time::Duration;

    use hyperindex_protocol::config::{WatchBackend, WatchConfig};
    use hyperindex_protocol::watch::NormalizedEventKind;
    use tempfile::tempdir;

    use super::WatcherService;

    #[test]
    fn watcher_observes_create_modify_remove_and_rename() {
        let tempdir = tempdir().unwrap();
        let repo_root = tempdir.path().join("repo");
        fs::create_dir_all(&repo_root).unwrap();
        let mut watcher = WatcherService::polling(&repo_root, watch_config(), Vec::new()).unwrap();

        spawn_after(40, {
            let repo_root = repo_root.clone();
            move || {
                fs::write(repo_root.join("alpha.txt"), "one\n").unwrap();
            }
        });
        let created = watcher.watch_once(Duration::from_millis(250)).unwrap();
        assert_eq!(created.events.len(), 1);
        assert_eq!(created.events[0].kind, NormalizedEventKind::Created);
        assert_eq!(created.events[0].path, "alpha.txt");

        spawn_after(40, {
            let repo_root = repo_root.clone();
            move || {
                fs::write(repo_root.join("alpha.txt"), "two\n").unwrap();
            }
        });
        let modified = watcher.watch_once(Duration::from_millis(250)).unwrap();
        assert_eq!(modified.events.len(), 1);
        assert_eq!(modified.events[0].kind, NormalizedEventKind::Modified);
        assert_eq!(modified.events[0].path, "alpha.txt");

        spawn_after(40, {
            let repo_root = repo_root.clone();
            move || {
                fs::rename(repo_root.join("alpha.txt"), repo_root.join("beta.txt")).unwrap();
            }
        });
        let renamed = watcher.watch_once(Duration::from_millis(250)).unwrap();
        assert_eq!(renamed.events.len(), 1);
        assert_eq!(renamed.events[0].kind, NormalizedEventKind::Renamed);
        assert_eq!(renamed.events[0].path, "beta.txt");
        assert_eq!(
            renamed.events[0].previous_path.as_deref(),
            Some("alpha.txt")
        );

        spawn_after(40, {
            let repo_root = repo_root.clone();
            move || {
                fs::remove_file(repo_root.join("beta.txt")).unwrap();
            }
        });
        let removed = watcher.watch_once(Duration::from_millis(250)).unwrap();
        assert_eq!(removed.events.len(), 1);
        assert_eq!(removed.events[0].kind, NormalizedEventKind::Removed);
        assert_eq!(removed.events[0].path, "beta.txt");
    }

    #[test]
    fn watcher_ignores_configured_and_temp_files() {
        let tempdir = tempdir().unwrap();
        let repo_root = tempdir.path().join("repo");
        fs::create_dir_all(repo_root.join("ignored")).unwrap();
        let mut watcher =
            WatcherService::polling(&repo_root, watch_config(), vec!["ignored/**".to_string()])
                .unwrap();

        spawn_after(40, {
            let repo_root = repo_root.clone();
            move || {
                fs::write(repo_root.join("ignored/noisy.txt"), "noise\n").unwrap();
                fs::write(repo_root.join(".#scratch"), "temp\n").unwrap();
            }
        });
        let result = watcher.watch_once(Duration::from_millis(250)).unwrap();
        assert!(result.events.is_empty());
    }

    #[test]
    fn watcher_coalesces_burst_modifications() {
        let tempdir = tempdir().unwrap();
        let repo_root = tempdir.path().join("repo");
        fs::create_dir_all(&repo_root).unwrap();
        fs::write(repo_root.join("burst.txt"), "one\n").unwrap();
        let mut watcher = WatcherService::polling(&repo_root, watch_config(), Vec::new()).unwrap();

        spawn_after(40, {
            let repo_root = repo_root.clone();
            move || {
                fs::write(repo_root.join("burst.txt"), "two\n").unwrap();
                thread::sleep(Duration::from_millis(15));
                fs::write(repo_root.join("burst.txt"), "three\n").unwrap();
                thread::sleep(Duration::from_millis(15));
                fs::write(repo_root.join("burst.txt"), "four\n").unwrap();
            }
        });
        let result = watcher.watch_once(Duration::from_millis(300)).unwrap();
        assert_eq!(result.events.len(), 1);
        assert_eq!(result.events[0].kind, NormalizedEventKind::Modified);
        assert_eq!(result.events[0].path, "burst.txt");
    }

    fn spawn_after(delay_ms: u64, action: impl FnOnce() + Send + 'static) {
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(delay_ms));
            action();
        });
    }

    fn watch_config() -> WatchConfig {
        WatchConfig {
            backend: WatchBackend::Poll,
            poll_interval_ms: 20,
            debounce_ms: 60,
            batch_max_events: 128,
        }
    }
}
