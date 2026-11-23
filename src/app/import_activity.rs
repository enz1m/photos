use std::{
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

use enzui_activity::{Job, Outcome, Progress, Publisher, Status, Unit, Visibility};

use super::Photos;
use crate::services::import::{ImportProgress, ImportReport};

#[derive(Default)]
pub(super) struct ImportActivity {
    publisher: Option<Publisher>,
    job: Option<Job>,
}

impl Photos {
    pub(super) fn begin_import(&mut self, total: usize) -> (u64, Arc<AtomicBool>) {
        self.busy = true;
        self.import_generation = self.import_generation.saturating_add(1);
        let cancel = Arc::new(AtomicBool::new(false));
        self.import_cancel = Some(Arc::clone(&cancel));
        self.import_progress_hidden = false;
        let progress = ImportProgress {
            completed: 0,
            total,
        };
        self.import_progress = Some(progress);

        self.import_activity.job = None;
        if self.import_activity.publisher.is_none() && !cfg!(test) {
            self.import_activity.publisher = Publisher::new("org.enzimos.Photos").ok();
        }
        if let Some(publisher) = &self.import_activity.publisher {
            self.import_activity.job = publisher
                .start_after(
                    Status {
                        progress: measurement(progress),
                        ..Status::running(enzui_iced_core::localization::text(
                            "photos-import-activity",
                        ))
                    },
                    Visibility::WhenUnfocused,
                    Duration::from_millis(800),
                )
                .ok();
        }
        (self.import_generation, cancel)
    }
}

impl ImportActivity {
    pub(super) fn progress(&self, progress: ImportProgress) {
        if let Some(job) = &self.job {
            let _ = job.set_progress(measurement(progress), None);
        }
    }

    pub(super) fn finish(&mut self, result: &Result<Arc<ImportReport>, String>) {
        if let Some(job) = self.job.take() {
            let outcome = match result {
                Ok(report) if report.cancelled => Outcome::Cancelled,
                Ok(report) if report.errors.is_empty() => Outcome::Succeeded,
                _ => Outcome::Failed,
            };
            let _ = job.finish(outcome);
        }
    }
}

fn measurement(progress: ImportProgress) -> Option<Progress> {
    (progress.total > 0).then_some(Progress {
        done: progress.completed as u64,
        total: progress.total as u64,
        unit: Unit::Items,
    })
}
