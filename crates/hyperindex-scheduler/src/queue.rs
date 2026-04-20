use hyperindex_protocol::status::SchedulerSnapshot;

use crate::jobs::{JobKind, JobRecord, JobState};
use crate::status::idle_status;

#[derive(Debug, Default)]
pub struct SchedulerService {
    next_job_id: u64,
    jobs: Vec<JobRecord>,
}

impl SchedulerService {
    pub fn new() -> Self {
        Self {
            next_job_id: 1,
            jobs: Vec::new(),
        }
    }

    pub fn enqueue(&mut self, repo_id: Option<&str>, kind: JobKind) -> String {
        let job_id = format!("job-{:06}", self.next_job_id);
        self.next_job_id += 1;
        self.jobs.push(JobRecord {
            job_id: job_id.clone(),
            repo_id: repo_id.map(str::to_string),
            kind,
            state: JobState::Pending,
        });
        job_id
    }

    pub fn mark_running(&mut self, job_id: &str) -> bool {
        self.transition(job_id, JobState::Running)
    }

    pub fn mark_succeeded(&mut self, job_id: &str) -> bool {
        self.transition(job_id, JobState::Succeeded)
    }

    pub fn mark_failed(&mut self, job_id: &str) -> bool {
        self.transition(job_id, JobState::Failed)
    }

    pub fn active_job_for_repo(&self, repo_id: &str) -> Option<String> {
        self.jobs.iter().find_map(|job| {
            (job.repo_id.as_deref() == Some(repo_id)
                && matches!(job.state, JobState::Pending | JobState::Running))
            .then(|| job.label())
        })
    }

    pub fn status(&self) -> SchedulerSnapshot {
        if self.jobs.is_empty() {
            return idle_status();
        }

        SchedulerSnapshot {
            mode: "local".to_string(),
            queue_depth: self
                .jobs
                .iter()
                .filter(|job| job.state == JobState::Pending)
                .count(),
            active_jobs: self
                .jobs
                .iter()
                .filter(|job| matches!(job.state, JobState::Pending | JobState::Running))
                .map(JobRecord::label)
                .collect(),
        }
    }

    fn transition(&mut self, job_id: &str, state: JobState) -> bool {
        if let Some(job) = self.jobs.iter_mut().find(|job| job.job_id == job_id) {
            job.state = state;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{jobs::JobKind, queue::SchedulerService};

    #[test]
    fn scheduler_tracks_pending_and_running_jobs() {
        let mut scheduler = SchedulerService::new();
        let job_id = scheduler.enqueue(Some("repo-1"), JobKind::RepoRefresh);
        let pending = scheduler.status();
        assert_eq!(pending.queue_depth, 1);
        assert_eq!(pending.active_jobs.len(), 1);
        assert!(pending.active_jobs[0].contains("repo_refresh"));

        assert!(scheduler.mark_running(&job_id));
        let running = scheduler.status();
        assert_eq!(running.queue_depth, 0);
        assert_eq!(running.active_jobs.len(), 1);
        assert_eq!(
            scheduler.active_job_for_repo("repo-1"),
            Some(running.active_jobs[0].clone())
        );

        assert!(scheduler.mark_succeeded(&job_id));
        let settled = scheduler.status();
        assert_eq!(settled.queue_depth, 0);
        assert!(settled.active_jobs.is_empty());
    }
}
