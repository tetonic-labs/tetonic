//! Dynamic task DAG validation and scheduling helpers.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use lokai_domain::{DependencyPolicy, RunSnapshot, TaskDependency, TaskId, TaskState};

pub fn would_create_cycle(
    dependencies: &BTreeMap<TaskId, Vec<TaskDependency>>,
    from: &TaskId,
    to: &TaskId,
) -> bool {
    if from == to {
        return true;
    }
    let mut adj: BTreeMap<TaskId, Vec<TaskId>> = BTreeMap::new();
    for (task, deps) in dependencies {
        for d in deps {
            adj.entry(task.clone())
                .or_default()
                .push(d.depends_on.clone());
        }
    }
    adj.entry(from.clone()).or_default().push(to.clone());
    has_cycle(&adj)
}

fn has_cycle(adj: &BTreeMap<TaskId, Vec<TaskId>>) -> bool {
    let mut visited = BTreeSet::new();
    let mut stack = BTreeSet::new();
    for node in adj.keys() {
        if dfs(node, adj, &mut visited, &mut stack) {
            return true;
        }
    }
    false
}

fn dfs(
    node: &TaskId,
    adj: &BTreeMap<TaskId, Vec<TaskId>>,
    visited: &mut BTreeSet<TaskId>,
    stack: &mut BTreeSet<TaskId>,
) -> bool {
    if stack.contains(node) {
        return true;
    }
    if visited.contains(node) {
        return false;
    }
    visited.insert(node.clone());
    stack.insert(node.clone());
    if let Some(neighbors) = adj.get(node) {
        for n in neighbors {
            if dfs(n, adj, visited, stack) {
                return true;
            }
        }
    }
    stack.remove(node);
    false
}

pub fn dependencies_satisfied(snapshot: &RunSnapshot, task_id: &TaskId) -> bool {
    let Some(deps) = snapshot.dependencies.get(task_id) else {
        return true;
    };
    for dep in deps {
        let Some(dep_task) = snapshot.tasks.get(&dep.depends_on) else {
            return false;
        };
        match dep.policy {
            DependencyPolicy::RequireSuccess => {
                if dep_task.state != TaskState::Succeeded {
                    return false;
                }
            }
            DependencyPolicy::AllowPartial => {
                if matches!(
                    dep_task.state,
                    TaskState::Created
                        | TaskState::Blocked
                        | TaskState::Ready
                        | TaskState::Leased
                        | TaskState::Running
                ) {
                    return false;
                }
            }
            DependencyPolicy::ContinueOnFailure => {
                if matches!(
                    dep_task.state,
                    TaskState::Created
                        | TaskState::Blocked
                        | TaskState::Ready
                        | TaskState::Leased
                        | TaskState::Running
                ) {
                    return false;
                }
            }
            DependencyPolicy::FallbackTask(_) => {
                if dep_task.state != TaskState::Succeeded && dep_task.state != TaskState::Failed {
                    return false;
                }
            }
        }
    }
    true
}

pub fn apply_dependency_failure(snapshot: &mut RunSnapshot, failed_task: &TaskId) {
    let deps_clone = snapshot.dependencies.clone();
    for (task_id, deps) in &deps_clone {
        for dep in deps {
            if &dep.depends_on != failed_task {
                continue;
            }
            let should_ready = match dep.policy {
                DependencyPolicy::AllowPartial | DependencyPolicy::ContinueOnFailure => {
                    snapshot
                        .tasks
                        .get(task_id)
                        .is_some_and(|t| t.state == TaskState::Blocked)
                        && dependencies_satisfied(snapshot, task_id)
                }
                _ => false,
            };
            let should_skip = matches!(
                dep.policy,
                DependencyPolicy::RequireSuccess | DependencyPolicy::FallbackTask(_)
            ) && snapshot
                .tasks
                .get(task_id)
                .is_some_and(|t| matches!(t.state, TaskState::Blocked | TaskState::Created));
            if should_skip {
                if let Some(task) = snapshot.tasks.get_mut(task_id) {
                    task.state = TaskState::Skipped;
                }
            }
            if let DependencyPolicy::FallbackTask(ref fallback) = dep.policy {
                if should_skip {
                    if let Some(fb) = snapshot.tasks.get_mut(fallback) {
                        if fb.state == TaskState::Blocked {
                            fb.state = TaskState::Ready;
                        }
                    }
                }
            }
            if should_ready {
                if let Some(task) = snapshot.tasks.get_mut(task_id) {
                    task.state = TaskState::Ready;
                }
            }
        }
    }
}

pub fn recompute_blocked_ready(snapshot: &mut RunSnapshot) {
    let task_ids: Vec<_> = snapshot.tasks.keys().cloned().collect();
    for task_id in task_ids {
        let Some(task) = snapshot.tasks.get(&task_id) else {
            continue;
        };
        if !matches!(
            task.state,
            TaskState::Created | TaskState::Blocked | TaskState::Ready
        ) {
            continue;
        }
        if dependencies_satisfied(snapshot, &task_id) {
            if let Some(task) = snapshot.tasks.get_mut(&task_id) {
                task.state = TaskState::Ready;
            }
        } else if let Some(task) = snapshot.tasks.get_mut(&task_id) {
            task.state = TaskState::Blocked;
        }
    }
}

pub fn task_is_executable(task_state: &TaskState) -> bool {
    matches!(
        task_state,
        TaskState::Created | TaskState::Blocked | TaskState::Ready
    )
}

pub fn task_is_locked(task_state: &TaskState) -> bool {
    matches!(task_state, TaskState::Leased | TaskState::Running)
}

pub fn topo_sort_tasks(snapshot: &RunSnapshot) -> Vec<TaskId> {
    let mut indegree: BTreeMap<TaskId, usize> = BTreeMap::new();
    for task_id in snapshot.tasks.keys() {
        indegree.entry(task_id.clone()).or_insert(0);
    }
    for (task_id, deps) in &snapshot.dependencies {
        indegree.entry(task_id.clone()).or_insert(0);
        for d in deps {
            indegree
                .entry(task_id.clone())
                .and_modify(|v| *v += 1)
                .or_insert(1);
            indegree.entry(d.depends_on.clone()).or_insert(0);
        }
    }
    let mut q: VecDeque<TaskId> = indegree
        .iter()
        .filter(|(_, &deg)| deg == 0)
        .map(|(k, _)| k.clone())
        .collect();
    let mut order = Vec::new();
    let mut adj: BTreeMap<TaskId, Vec<TaskId>> = BTreeMap::new();
    for (task_id, deps) in &snapshot.dependencies {
        for d in deps {
            adj.entry(d.depends_on.clone())
                .or_default()
                .push(task_id.clone());
        }
    }
    while let Some(n) = q.pop_front() {
        order.push(n.clone());
        if let Some(children) = adj.get(&n) {
            for c in children {
                if let Some(deg) = indegree.get_mut(c) {
                    *deg = deg.saturating_sub(1);
                    if *deg == 0 {
                        q.push_back(c.clone());
                    }
                }
            }
        }
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;
    use lokai_domain::{RunId, SessionId, TaskInputBinding, TaskRecord};

    #[test]
    fn self_dependency_is_cycle() {
        let a = TaskId::new("a");
        let deps = BTreeMap::new();
        assert!(would_create_cycle(&deps, &a, &a));
    }

    #[test]
    fn recompute_demotes_ready_when_dependency_added() {
        use crate::replay::empty_snapshot;
        let dep = TaskId::new("dep");
        let consumer = TaskId::new("consumer");
        let mut snap = empty_snapshot(RunId::new("run"), Some(SessionId::new("sess")));
        snap.state = lokai_domain::RunState::Active;
        snap.sequence = 1;
        snap.tasks.insert(
            dep.clone(),
            TaskRecord {
                task_id: dep.clone(),
                state: TaskState::Ready,
                binding: TaskInputBinding::default(),
                accepted_artifact: None,
                active_attempt: None,
                winning_attempt: None,
                finalization_claim: None,
                completed_version: None,
                retry: Default::default(),
                side_effect_keys: Vec::new(),
            },
        );
        snap.tasks.insert(
            consumer.clone(),
            TaskRecord {
                task_id: consumer.clone(),
                state: TaskState::Ready,
                binding: TaskInputBinding::default(),
                accepted_artifact: None,
                active_attempt: None,
                winning_attempt: None,
                finalization_claim: None,
                completed_version: None,
                retry: Default::default(),
                side_effect_keys: Vec::new(),
            },
        );
        snap.dependencies.insert(
            consumer.clone(),
            vec![TaskDependency {
                depends_on: dep.clone(),
                policy: DependencyPolicy::RequireSuccess,
            }],
        );
        recompute_blocked_ready(&mut snap);
        assert_eq!(snap.tasks.get(&consumer).unwrap().state, TaskState::Blocked);
    }
}
