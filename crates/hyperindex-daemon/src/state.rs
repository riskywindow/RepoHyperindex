use std::collections::{BTreeMap, VecDeque};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use hyperindex_config::LoadedConfig;
use hyperindex_core::{HyperindexError, HyperindexResult};
use hyperindex_git_state::GitInspector;
use hyperindex_protocol::repo::{RepoRecord, RepoStatusResponse};
use hyperindex_protocol::status::{DaemonLifecycleState, RuntimeStatus, TransportSummary};
use hyperindex_protocol::watch::{WatchBatch, WatcherStatus};
use hyperindex_protocol::{CONFIG_VERSION, PROTOCOL_VERSION};
use hyperindex_repo_store::RepoStore;
use hyperindex_scheduler::{SchedulerService, jobs::JobKind};
use hyperindex_snapshot::{
    SnapshotAssembler, build_base_snapshot, build_buffer_overlays, build_working_tree_overlay,
};
use hyperindex_watcher::{PollingWatcher, WatcherService};
use tokio::sync::watch;

use crate::impact::scan_impact_runtime_status;
use crate::semantic::scan_semantic_runtime_status;
use crate::symbols::{scan_parse_runtime_status, scan_symbol_runtime_status};

#[derive(Debug)]
struct ManagedWatcher {
    status: WatcherStatus,
    #[allow(dead_code)]
    service: WatcherService<PollingWatcher>,
}

#[derive(Debug)]
struct StateInner {
    lifecycle: DaemonLifecycleState,
    pid: Option<u32>,
    connected_clients: usize,
    watchers: BTreeMap<String, ManagedWatcher>,
    event_queue: VecDeque<WatchBatch>,
    last_error_code: BTreeMap<String, String>,
}

#[derive(Debug)]
pub struct DaemonStateManager {
    loaded_config: LoadedConfig,
    scheduler: Mutex<SchedulerService>,
    inner: Mutex<StateInner>,
    snapshot_assembler: SnapshotAssembler,
    shutdown_tx: watch::Sender<bool>,
}

impl DaemonStateManager {
    pub fn new(loaded_config: LoadedConfig) -> Arc<Self> {
        let (shutdown_tx, _) = watch::channel(false);
        Arc::new(Self {
            loaded_config,
            scheduler: Mutex::new(SchedulerService::new()),
            inner: Mutex::new(StateInner {
                lifecycle: DaemonLifecycleState::Starting,
                pid: None,
                connected_clients: 0,
                watchers: BTreeMap::new(),
                event_queue: VecDeque::new(),
                last_error_code: BTreeMap::new(),
            }),
            snapshot_assembler: SnapshotAssembler,
            shutdown_tx,
        })
    }

    pub fn loaded_config(&self) -> &LoadedConfig {
        &self.loaded_config
    }

    pub fn snapshot_assembler(&self) -> &SnapshotAssembler {
        &self.snapshot_assembler
    }

    pub fn shutdown_receiver(&self) -> watch::Receiver<bool> {
        self.shutdown_tx.subscribe()
    }

    pub fn request_shutdown(&self) -> bool {
        if let Ok(mut inner) = self.inner_mut() {
            inner.lifecycle = DaemonLifecycleState::Stopping;
        }
        let _ = self.shutdown_tx.send(true);
        true
    }

    pub fn set_lifecycle(&self, lifecycle: DaemonLifecycleState) -> HyperindexResult<()> {
        self.inner_mut()?.lifecycle = lifecycle;
        Ok(())
    }

    pub fn lifecycle(&self) -> HyperindexResult<DaemonLifecycleState> {
        Ok(self.inner()?.lifecycle.clone())
    }

    pub fn set_pid(&self, pid: u32) -> HyperindexResult<()> {
        self.inner_mut()?.pid = Some(pid);
        Ok(())
    }

    pub fn clear_pid(&self) -> HyperindexResult<()> {
        self.inner_mut()?.pid = None;
        Ok(())
    }

    pub fn connected_client_opened(&self) -> HyperindexResult<()> {
        self.inner_mut()?.connected_clients += 1;
        Ok(())
    }

    pub fn connected_client_closed(&self) -> HyperindexResult<()> {
        let inner = &mut *self.inner_mut()?;
        if inner.connected_clients > 0 {
            inner.connected_clients -= 1;
        }
        Ok(())
    }

    pub fn open_store(&self) -> HyperindexResult<RepoStore> {
        RepoStore::open_from_config(&self.loaded_config.config)
    }

    pub fn enqueue_job(&self, repo_id: Option<&str>, kind: JobKind) -> HyperindexResult<String> {
        Ok(self.scheduler_mut()?.enqueue(repo_id, kind))
    }

    pub fn mark_job_running(&self, job_id: &str) -> HyperindexResult<()> {
        self.scheduler_mut()?.mark_running(job_id);
        Ok(())
    }

    pub fn mark_job_succeeded(&self, job_id: &str) -> HyperindexResult<()> {
        self.scheduler_mut()?.mark_succeeded(job_id);
        Ok(())
    }

    pub fn mark_job_failed(&self, job_id: &str) -> HyperindexResult<()> {
        self.scheduler_mut()?.mark_failed(job_id);
        Ok(())
    }

    pub fn active_job_for_repo(&self, repo_id: &str) -> HyperindexResult<Option<String>> {
        Ok(self.scheduler()?.active_job_for_repo(repo_id))
    }

    pub fn runtime_status(&self) -> HyperindexResult<RuntimeStatus> {
        let store = self.open_store()?;
        let summary = store.summary()?;
        let scheduler = self.scheduler()?.status();
        let inner = self.inner()?;
        Ok(RuntimeStatus {
            protocol_version: PROTOCOL_VERSION.to_string(),
            config_version: CONFIG_VERSION,
            runtime_root: self
                .loaded_config
                .config
                .directories
                .runtime_root
                .display()
                .to_string(),
            state_dir: self
                .loaded_config
                .config
                .directories
                .state_dir
                .display()
                .to_string(),
            socket_path: self
                .loaded_config
                .config
                .transport
                .socket_path
                .display()
                .to_string(),
            daemon_state: inner.lifecycle.clone(),
            pid: inner.pid,
            transport: TransportSummary {
                kind: self.loaded_config.config.transport.kind.clone(),
                socket_path: Some(
                    self.loaded_config
                        .config
                        .transport
                        .socket_path
                        .display()
                        .to_string(),
                ),
                connected_clients: inner.connected_clients,
            },
            repo_count: summary.repo_count,
            manifest_count: summary.manifest_count,
            scheduler,
            parser: Some(scan_parse_runtime_status(&self.loaded_config)?),
            symbol_index: Some(scan_symbol_runtime_status(&self.loaded_config)?),
            impact: Some(scan_impact_runtime_status(&self.loaded_config)?),
            semantic: Some(scan_semantic_runtime_status(&self.loaded_config)?),
        })
    }

    pub fn build_repo_status(&self, repo_id: &str) -> HyperindexResult<RepoStatusResponse> {
        let store = self.open_store()?;
        let repo = store.show_repo(repo_id)?;
        ensure_repo_root_exists(&repo)?;
        let git_state = GitInspector.inspect_repo(&repo.repo_root)?;
        let dirty_path_count = git_state.working_tree.dirty_tracked_files.len()
            + git_state.working_tree.untracked_files.len()
            + git_state.working_tree.deleted_files.len()
            + git_state.working_tree.renamed_files.len();
        let watch_attached = self
            .inner()?
            .watchers
            .get(repo_id)
            .map(|watcher| watcher.status.attached)
            .unwrap_or(false);
        let active_job = self.active_job_for_repo(repo_id)?;
        let last_error_code = self.inner()?.last_error_code.get(repo_id).cloned();

        Ok(RepoStatusResponse {
            repo_id: repo.repo_id,
            repo_root: repo.repo_root,
            display_name: repo.display_name,
            branch: git_state.branch,
            head_commit: git_state.head_commit,
            working_tree_digest: git_state.working_tree.digest.clone(),
            is_dirty: dirty_path_count > 0,
            watch_attached,
            dirty_path_count,
            dirty_tracked_files: git_state.working_tree.dirty_tracked_files,
            untracked_files: git_state.working_tree.untracked_files,
            deleted_files: git_state.working_tree.deleted_files,
            renamed_files: git_state.working_tree.renamed_files,
            ignored_files: git_state.working_tree.ignored_files,
            last_snapshot_id: repo.last_snapshot_id,
            active_job,
            last_error_code,
        })
    }

    pub fn attach_watcher(&self, repo: &RepoRecord) -> HyperindexResult<()> {
        ensure_repo_root_exists(repo)?;
        let mut inner = self.inner_mut()?;
        if inner.watchers.contains_key(&repo.repo_id) {
            return Ok(());
        }
        let service = WatcherService::polling(
            Path::new(&repo.repo_root),
            self.loaded_config.config.watch.clone(),
            self.effective_ignore_patterns(repo),
        )?;
        inner.watchers.insert(
            repo.repo_id.clone(),
            ManagedWatcher {
                status: WatcherStatus {
                    repo_id: repo.repo_id.clone(),
                    attached: true,
                    backend: "poll".to_string(),
                    last_sequence: None,
                    dropped_events: 0,
                },
                service,
            },
        );
        Ok(())
    }

    pub fn detach_watcher(&self, repo_id: &str) -> HyperindexResult<()> {
        self.inner_mut()?.watchers.remove(repo_id);
        Ok(())
    }

    pub fn push_watch_batch(&self, batch: WatchBatch) -> HyperindexResult<()> {
        let inner = &mut *self.inner_mut()?;
        inner.event_queue.push_back(batch);
        while inner.event_queue.len() > 256 {
            inner.event_queue.pop_front();
        }
        Ok(())
    }

    pub fn create_snapshot(
        &self,
        repo: &RepoRecord,
        include_working_tree: bool,
        buffer_ids: &[String],
    ) -> HyperindexResult<hyperindex_protocol::snapshot::ComposedSnapshot> {
        ensure_repo_root_exists(repo)?;
        let store = self.open_store()?;
        let git_state = GitInspector.inspect_repo(&repo.repo_root)?;
        let head_commit = git_state.head_commit.clone().ok_or_else(|| {
            HyperindexError::Message(
                format!(
                    "snapshot create requires a git HEAD commit for repo {}; commit the repo first or remove the repo from the runtime if the path is no longer valid",
                    repo.repo_id
                ),
            )
        })?;
        let base = build_base_snapshot(Path::new(&repo.repo_root), &head_commit)?;
        let working_tree = if include_working_tree {
            build_working_tree_overlay(Path::new(&repo.repo_root), &git_state)?
        } else {
            hyperindex_protocol::snapshot::WorkingTreeOverlay {
                digest: hyperindex_snapshot::base::digest_snapshot_components(std::iter::empty()),
                entries: Vec::new(),
            }
        };
        let stored_buffers = store.load_buffers(&repo.repo_id, buffer_ids)?;
        let buffer_overlays = build_buffer_overlays(&stored_buffers)?;
        let snapshot = self.snapshot_assembler.compose(
            &repo.repo_id,
            &repo.repo_root,
            base,
            working_tree,
            buffer_overlays,
        )?;
        store.persist_manifest(&snapshot)?;
        Ok(snapshot)
    }

    pub fn set_repo_error_code(
        &self,
        repo_id: impl Into<String>,
        error_code: impl Into<String>,
    ) -> HyperindexResult<()> {
        self.inner_mut()?
            .last_error_code
            .insert(repo_id.into(), error_code.into());
        Ok(())
    }

    pub fn clear_repo_error_code(&self, repo_id: &str) -> HyperindexResult<()> {
        self.inner_mut()?.last_error_code.remove(repo_id);
        Ok(())
    }

    fn effective_ignore_patterns(&self, repo: &RepoRecord) -> Vec<String> {
        let mut patterns = self.loaded_config.config.ignores.global_patterns.clone();
        patterns.extend(repo.ignore_settings.patterns.clone());
        patterns
    }

    fn scheduler(&self) -> HyperindexResult<MutexGuard<'_, SchedulerService>> {
        self.scheduler
            .lock()
            .map_err(|_| HyperindexError::Message("scheduler mutex was poisoned".to_string()))
    }

    fn scheduler_mut(&self) -> HyperindexResult<MutexGuard<'_, SchedulerService>> {
        self.scheduler()
    }

    fn inner(&self) -> HyperindexResult<MutexGuard<'_, StateInner>> {
        self.inner
            .lock()
            .map_err(|_| HyperindexError::Message("daemon state mutex was poisoned".to_string()))
    }

    fn inner_mut(&self) -> HyperindexResult<MutexGuard<'_, StateInner>> {
        self.inner()
    }
}

pub fn short_watch_timeout() -> Duration {
    Duration::from_millis(25)
}

fn ensure_repo_root_exists(repo: &RepoRecord) -> HyperindexResult<()> {
    if Path::new(&repo.repo_root).exists() {
        return Ok(());
    }

    Err(HyperindexError::Message(format!(
        "repo {} root is missing at {}; restore the repo path or remove it with `hyperctl repos remove --repo-id {}`",
        repo.repo_id, repo.repo_root, repo.repo_id
    )))
}
