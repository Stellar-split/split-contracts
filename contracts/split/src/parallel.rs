//! Parallel operation scheduling (issue #1019).
//!
//! Soroban executes a contract's operations sequentially, so this module does
//! not spawn threads. It plans how a batch of operations *would* be split
//! across independent lanes for an executor that can run lanes concurrently
//! (an off-chain batcher, or several sub-invoices), and verifies that the plan
//! is safe and the combined result is correct. Heap free and `no_std` friendly.
//!
//! - **Work distribution & load balancing** — [`schedule`] assigns each task
//!   to the least-loaded lane.
//! - **Synchronization** — tasks that share a `key` are pinned to one lane and
//!   keep their relative order, so concurrent lanes never touch the same key.
//! - **Result aggregation** — [`execute`] runs lane by lane and returns the
//!   results in the original task order with checked totals.
//! - **Metrics** — [`ScheduleMetrics`] reports speedup and balance.

/// Maximum number of tasks per schedule.
pub const MAX_TASKS: usize = 64;
/// Maximum number of lanes.
pub const MAX_LANES: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParallelError {
    /// `lanes` is zero or exceeds [`MAX_LANES`].
    InvalidLanes,
    /// More than [`MAX_TASKS`] tasks, or no tasks.
    InvalidTaskCount,
    /// A lane total overflowed `i128`.
    Overflow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Task {
    pub id: u32,
    /// Tasks with the same key conflict and run on the same lane, in order.
    pub key: u64,
    /// Estimated cost, used for load balancing.
    pub cost: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct Schedule {
    lane_of: [u8; MAX_TASKS],
    task_count: usize,
    lane_cost: [u64; MAX_LANES],
    lanes: usize,
}

impl Schedule {
    pub fn lanes(&self) -> usize {
        self.lanes
    }

    pub fn lane_of(&self, task_index: usize) -> Option<usize> {
        if task_index < self.task_count {
            Some(self.lane_of[task_index] as usize)
        } else {
            None
        }
    }

    pub fn lane_cost(&self, lane: usize) -> u64 {
        if lane < self.lanes {
            self.lane_cost[lane]
        } else {
            0
        }
    }

    pub fn metrics(&self) -> ScheduleMetrics {
        let costs = &self.lane_cost[..self.lanes];
        let total: u64 = costs.iter().sum();
        let makespan = costs.iter().copied().max().unwrap_or(0);
        let min_busy = costs.iter().copied().filter(|c| *c > 0).min().unwrap_or(0);
        ScheduleMetrics {
            tasks: self.task_count as u32,
            total_cost: total,
            makespan,
            speedup_bps: if makespan == 0 { 0 } else { (total * 10_000 / makespan) as u32 },
            imbalance_bps: if makespan == 0 {
                0
            } else {
                ((makespan - min_busy) * 10_000 / makespan) as u32
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScheduleMetrics {
    pub tasks: u32,
    /// Sum of every task's cost (the sequential runtime).
    pub total_cost: u64,
    /// Cost of the busiest lane (the parallel runtime).
    pub makespan: u64,
    /// `total_cost / makespan` in basis points; 10_000 means no speedup.
    pub speedup_bps: u32,
    /// Gap between the busiest and least busy lane, in basis points of the makespan.
    pub imbalance_bps: u32,
}

/// Assign tasks to `lanes` lanes. Tasks sharing a key stay together; new keys
/// go to the currently least-loaded lane (lowest index wins ties).
pub fn schedule(tasks: &[Task], lanes: usize) -> Result<Schedule, ParallelError> {
    if lanes == 0 || lanes > MAX_LANES {
        return Err(ParallelError::InvalidLanes);
    }
    if tasks.is_empty() || tasks.len() > MAX_TASKS {
        return Err(ParallelError::InvalidTaskCount);
    }

    let mut plan = Schedule {
        lane_of: [0; MAX_TASKS],
        task_count: tasks.len(),
        lane_cost: [0; MAX_LANES],
        lanes,
    };

    for (i, task) in tasks.iter().enumerate() {
        let pinned = tasks[..i].iter().position(|earlier| earlier.key == task.key);
        let lane = match pinned {
            Some(earlier) => plan.lane_of[earlier] as usize,
            None => {
                let mut best = 0;
                for l in 1..lanes {
                    if plan.lane_cost[l] < plan.lane_cost[best] {
                        best = l;
                    }
                }
                best
            }
        };
        plan.lane_of[i] = lane as u8;
        plan.lane_cost[lane] += task.cost as u64;
    }
    Ok(plan)
}

/// Results of running a schedule.
#[derive(Clone, Copy, Debug)]
pub struct Execution {
    results: [i128; MAX_TASKS],
    count: usize,
    lane_totals: [i128; MAX_LANES],
    lanes: usize,
    /// Sum of every result.
    pub total: i128,
}

impl Execution {
    /// Results in the original task order.
    pub fn results(&self) -> &[i128] {
        &self.results[..self.count]
    }

    pub fn lane_totals(&self) -> &[i128] {
        &self.lane_totals[..self.lanes]
    }
}

/// Run `work` for every task, one lane at a time, and aggregate the results.
///
/// Within a lane tasks run in their original order, so tasks sharing a key
/// observe each other's effects exactly as in a sequential run.
pub fn execute<F>(tasks: &[Task], plan: &Schedule, mut work: F) -> Result<Execution, ParallelError>
where
    F: FnMut(&Task) -> i128,
{
    let mut out = Execution {
        results: [0; MAX_TASKS],
        count: tasks.len(),
        lane_totals: [0; MAX_LANES],
        lanes: plan.lanes,
        total: 0,
    };

    for lane in 0..plan.lanes {
        for (i, task) in tasks.iter().enumerate() {
            if plan.lane_of[i] as usize != lane {
                continue;
            }
            let result = work(task);
            out.results[i] = result;
            out.lane_totals[lane] = out.lane_totals[lane]
                .checked_add(result)
                .ok_or(ParallelError::Overflow)?;
        }
    }

    for lane in 0..plan.lanes {
        out.total = out.total.checked_add(out.lane_totals[lane]).ok_or(ParallelError::Overflow)?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: u32, key: u64, cost: u32) -> Task {
        Task { id, key, cost }
    }

    #[test]
    fn rejects_invalid_inputs() {
        let t = [task(1, 1, 1)];
        assert_eq!(schedule(&t, 0).err(), Some(ParallelError::InvalidLanes));
        assert_eq!(schedule(&t, MAX_LANES + 1).err(), Some(ParallelError::InvalidLanes));
        assert_eq!(schedule(&[], 2).err(), Some(ParallelError::InvalidTaskCount));
        let many = [task(0, 0, 1); MAX_TASKS + 1];
        assert_eq!(schedule(&many, 2).err(), Some(ParallelError::InvalidTaskCount));
    }

    #[test]
    fn independent_tasks_are_balanced_across_lanes() {
        let tasks = [task(0, 0, 10), task(1, 1, 10), task(2, 2, 10), task(3, 3, 10)];
        let plan = schedule(&tasks, 2).unwrap();
        assert_eq!(plan.lane_cost(0), 20);
        assert_eq!(plan.lane_cost(1), 20);

        let m = plan.metrics();
        assert_eq!(m.total_cost, 40);
        assert_eq!(m.makespan, 20);
        assert_eq!(m.speedup_bps, 20_000);
        assert_eq!(m.imbalance_bps, 0);
    }

    #[test]
    fn expensive_task_goes_alone_and_cheap_ones_fill_the_other_lane() {
        let tasks = [task(0, 0, 100), task(1, 1, 10), task(2, 2, 10), task(3, 3, 10)];
        let plan = schedule(&tasks, 2).unwrap();
        assert_eq!(plan.lane_of(0), Some(0));
        assert_eq!(plan.lane_of(1), Some(1));
        assert_eq!(plan.lane_of(2), Some(1));
        assert_eq!(plan.lane_of(3), Some(1));
        assert_eq!(plan.lane_of(4), None);
        assert_eq!(plan.metrics().makespan, 100);
    }

    #[test]
    fn conflicting_tasks_share_a_lane() {
        let tasks = [task(0, 7, 5), task(1, 8, 5), task(2, 7, 5), task(3, 9, 5), task(4, 7, 5)];
        let plan = schedule(&tasks, 3).unwrap();
        let lane = plan.lane_of(0).unwrap();
        assert_eq!(plan.lane_of(2), Some(lane));
        assert_eq!(plan.lane_of(4), Some(lane));
        assert_eq!(plan.lane_cost(lane), 15);
    }

    #[test]
    fn execution_matches_sequential_result_and_preserves_order() {
        let tasks = [task(0, 1, 1), task(1, 2, 1), task(2, 1, 1), task(3, 3, 1), task(4, 2, 1)];

        // Sequential reference: a per-key running counter.
        let mut counters = [0i128; 4];
        let mut expected = [0i128; 5];
        for (i, t) in tasks.iter().enumerate() {
            counters[t.key as usize] += 10;
            expected[i] = counters[t.key as usize];
        }

        let plan = schedule(&tasks, 3).unwrap();
        let mut parallel_counters = [0i128; 4];
        let exec = execute(&tasks, &plan, |t| {
            parallel_counters[t.key as usize] += 10;
            parallel_counters[t.key as usize]
        })
        .unwrap();

        assert_eq!(exec.results(), &expected);
        assert_eq!(exec.total, expected.iter().sum::<i128>());
        assert_eq!(exec.lane_totals().iter().sum::<i128>(), exec.total);
        assert_eq!(exec.lane_totals().len(), 3);
    }

    #[test]
    fn aggregation_reports_overflow() {
        let tasks = [task(0, 1, 1), task(1, 2, 1)];
        let plan = schedule(&tasks, 1).unwrap();
        let err = execute(&tasks, &plan, |_| i128::MAX).err();
        assert_eq!(err, Some(ParallelError::Overflow));

        let plan = schedule(&tasks, 2).unwrap();
        let err = execute(&tasks, &plan, |_| i128::MAX).err();
        assert_eq!(err, Some(ParallelError::Overflow));
    }

    #[test]
    fn single_lane_has_no_speedup() {
        let tasks = [task(0, 1, 5), task(1, 2, 5)];
        let plan = schedule(&tasks, 1).unwrap();
        assert_eq!(plan.metrics().speedup_bps, 10_000);
        assert_eq!(plan.lanes(), 1);
    }
}
