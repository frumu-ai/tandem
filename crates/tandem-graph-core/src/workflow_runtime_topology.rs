use crate::WorkflowStepDependencySummary;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn parallel_groups(
    dependencies: &[(String, WorkflowStepDependencySummary)],
) -> Vec<Vec<String>> {
    let depths = step_depths(dependencies);
    let mut groups: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    for (step_id, _) in dependencies {
        groups
            .entry(*depths.get(step_id).unwrap_or(&0))
            .or_default()
            .push(step_id.clone());
    }
    groups.into_values().collect()
}

pub(crate) fn critical_path(
    dependencies: &[(String, WorkflowStepDependencySummary)],
) -> Vec<String> {
    let depths = step_depths(dependencies);
    let Some((mut current, _)) = depths.iter().max_by_key(|(_, depth)| *depth) else {
        return Vec::new();
    };
    let by_step: BTreeMap<_, _> = dependencies
        .iter()
        .map(|(step_id, summary)| (step_id.as_str(), summary))
        .collect();
    let mut path = Vec::new();
    loop {
        path.push(current.clone());
        let Some(summary) = by_step.get(current.as_str()) else {
            break;
        };
        let Some(next) = summary
            .depends_on
            .iter()
            .max_by_key(|dependency| depths.get(*dependency).copied().unwrap_or(0))
        else {
            break;
        };
        current = next;
    }
    path.reverse();
    path
}

fn step_depths(
    dependencies: &[(String, WorkflowStepDependencySummary)],
) -> BTreeMap<String, usize> {
    let by_step: BTreeMap<_, _> = dependencies
        .iter()
        .map(|(step_id, summary)| (step_id.as_str(), summary))
        .collect();
    let mut depths = BTreeMap::new();
    for (step_id, _) in dependencies {
        let depth = step_depth(step_id, &by_step, &mut depths, &mut BTreeSet::new());
        depths.insert(step_id.clone(), depth);
    }
    depths
}

fn step_depth(
    step_id: &str,
    by_step: &BTreeMap<&str, &WorkflowStepDependencySummary>,
    depths: &mut BTreeMap<String, usize>,
    visiting: &mut BTreeSet<String>,
) -> usize {
    if let Some(depth) = depths.get(step_id) {
        return *depth;
    }
    if !visiting.insert(step_id.to_string()) {
        return 0;
    }
    let depth = by_step
        .get(step_id)
        .map(|summary| {
            summary
                .depends_on
                .iter()
                .map(|dependency| step_depth(dependency, by_step, depths, visiting) + 1)
                .max()
                .unwrap_or(0)
        })
        .unwrap_or(0);
    visiting.remove(step_id);
    depths.insert(step_id.to_string(), depth);
    depth
}
