//! `DeadlineBoundedFanout` — generic, port-free mechanism that drains a
//! [`JoinSet`] under a wall-clock deadline.
//!
//! It owns the single trickiest, most-churned (#785 / #886) and most
//! reusable concurrency concern in the dispatch path: run N tasks, hand
//! each settled result to a sink as it lands, and once the deadline
//! elapses stop waiting and return whatever is still in flight so the
//! caller can drive it to completion off the hot path.
//!
//! ## Contract red line — VISION locked decision #59 (clipboard transience)
//!
//! This is a *deadline + observe* mechanism, NOT a queue / retry /
//! redelivery mechanism. Tasks run exactly once; the deadline only bounds
//! how long the FOREGROUND waits for them. Tasks still in flight at the
//! deadline are handed back so the caller can *finish and record* them —
//! never replayed. Do not grow retry / backoff / persistence into this
//! type: automatic redelivery is an absolute project禁区.

use std::time::{Duration, Instant};

use tokio::task::{JoinError, JoinSet};

/// Drains a `JoinSet` for at most `deadline`, yielding still-running tasks
/// back to the caller when the deadline elapses.
pub(crate) struct DeadlineBoundedFanout {
    deadline: Duration,
}

impl DeadlineBoundedFanout {
    pub(crate) fn new(deadline: Duration) -> Self {
        Self { deadline }
    }

    /// Drain `set` until the deadline elapses or every task settles,
    /// handing each foreground settle (including the `Err(JoinError)` of a
    /// panicked / cancelled task) to `on_settled` as it lands — so a sink
    /// that timestamps results observes each task's individual completion.
    ///
    /// Returns the still-running `JoinSet` (possibly empty) for the caller
    /// to drive in the background. If the deadline has already elapsed when
    /// called, no task is awaited and the whole set is returned untouched.
    pub(crate) async fn drain_foreground<R>(
        &self,
        mut set: JoinSet<R>,
        mut on_settled: impl FnMut(Result<R, JoinError>),
    ) -> JoinSet<R>
    where
        R: Send + 'static,
    {
        let started = Instant::now();
        loop {
            let remaining = self.deadline.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                break;
            }
            match tokio::time::timeout(remaining, set.join_next()).await {
                Ok(Some(joined)) => on_settled(joined),
                Ok(None) => break, // set drained — all tasks settled within deadline
                Err(_) => break,   // deadline elapsed — defer the remainder
            }
        }
        set
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::time::sleep;

    #[tokio::test]
    async fn returns_empty_set_when_all_tasks_settle_within_deadline() {
        let fanout = DeadlineBoundedFanout::new(Duration::from_secs(5));
        let mut set: JoinSet<u32> = JoinSet::new();
        for i in 0..3 {
            set.spawn(async move { i });
        }
        let mut settled = Vec::new();
        let leftover = fanout
            .drain_foreground(set, |r| settled.push(r.unwrap()))
            .await;
        settled.sort_unstable();
        assert_eq!(settled, vec![0, 1, 2]);
        assert_eq!(leftover.len(), 0);
    }

    #[tokio::test]
    async fn hands_back_in_flight_tasks_when_deadline_elapses() {
        let fanout = DeadlineBoundedFanout::new(Duration::from_millis(50));
        let mut set: JoinSet<u32> = JoinSet::new();
        set.spawn(async { 1 }); // settles fast
        set.spawn(async {
            sleep(Duration::from_secs(10)).await;
            2
        }); // outlives the deadline
        let mut settled = Vec::new();
        let leftover = fanout
            .drain_foreground(set, |r| settled.push(r.unwrap()))
            .await;
        assert_eq!(settled, vec![1]);
        assert_eq!(leftover.len(), 1);
    }

    #[tokio::test]
    async fn surfaces_join_error_for_panicked_task() {
        let fanout = DeadlineBoundedFanout::new(Duration::from_secs(5));
        let mut set: JoinSet<u32> = JoinSet::new();
        set.spawn(async { panic!("boom") });
        let mut errors = 0usize;
        let leftover = fanout
            .drain_foreground(set, |r| {
                if r.is_err() {
                    errors += 1;
                }
            })
            .await;
        assert_eq!(errors, 1);
        assert_eq!(leftover.len(), 0);
    }
}
