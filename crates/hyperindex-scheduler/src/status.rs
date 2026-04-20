use hyperindex_protocol::status::SchedulerSnapshot;

pub fn idle_status() -> SchedulerSnapshot {
    SchedulerSnapshot {
        mode: "local".to_string(),
        queue_depth: 0,
        active_jobs: Vec::new(),
    }
}
