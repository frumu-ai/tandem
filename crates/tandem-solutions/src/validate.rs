// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use crate::*;
use semver::{Version, VersionReq};
use std::collections::{BTreeMap, BTreeSet};

pub fn parse_blueprint(input: &str) -> Result<SolutionBlueprint, SolutionError> {
    if input.len() > MAX_BLUEPRINT_BYTES {
        return Err(SolutionError::new(
            "size_limit",
            "$",
            "Blueprint exceeds 1 MiB",
        ));
    }
    // Value rejects duplicate YAML keys before typed map deserialization could
    // silently keep the last entry. JSON is also valid YAML input.
    let value: serde_yaml::Value = serde_yaml::from_str(input)
        .map_err(|_| SolutionError::new("parse_error", "$", "Invalid document or duplicate key"))?;
    if value.get("schema_version").and_then(|v| v.as_str()) != Some(SCHEMA_VERSION) {
        return Err(SolutionError::new(
            "unsupported_schema",
            "schema_version",
            "Expected string version 1; migrate other versions explicitly",
        ));
    }
    let blueprint: SolutionBlueprint = serde_yaml::from_value(value)
        .map_err(|_| SolutionError::new("invalid_schema", "$", "Unknown field, missing field or invalid type; validate against solution-blueprint.schema.json"))?;
    validate_blueprint(&blueprint)?;
    Ok(blueprint)
}

pub fn blueprint_hash(blueprint: &SolutionBlueprint) -> Result<String, SolutionError> {
    validate_blueprint(blueprint)?;
    Ok(sha256(&canonical_json(blueprint)?))
}

pub(crate) fn identifier(value: &str, path: &str) -> Result<(), SolutionError> {
    if value.is_empty()
        || value.len() > 80
        || !value.as_bytes()[0].is_ascii_alphanumeric()
        || !value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
    {
        return Err(SolutionError::new("invalid_id", path, "Use 1-80 lowercase letters, digits, dots, underscores or hyphens, starting with a letter or digit"));
    }
    Ok(())
}

pub(crate) fn digest(value: &str, path: &str) -> Result<(), SolutionError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(SolutionError::new(
            "invalid_digest",
            path,
            "Expected lowercase 64-character SHA-256",
        ));
    }
    Ok(())
}

pub(crate) fn version(value: &str, path: &str) -> Result<Version, SolutionError> {
    Version::parse(value).map_err(|_| {
        SolutionError::new(
            "invalid_version",
            path,
            "Expected exact semver, for example 1.0.0",
        )
    })
}

pub(crate) fn requirement(value: &str, path: &str) -> Result<VersionReq, SolutionError> {
    VersionReq::parse(value).map_err(|_| {
        SolutionError::new(
            "invalid_version_requirement",
            path,
            "Expected a semver requirement",
        )
    })
}

pub fn validate_blueprint(blueprint: &SolutionBlueprint) -> Result<(), SolutionError> {
    if blueprint.schema_version != SCHEMA_VERSION {
        return Err(SolutionError::new(
            "unsupported_schema",
            "schema_version",
            "Expected string version 1",
        ));
    }
    identifier(&blueprint.solution.id, "solution.id")?;
    version(&blueprint.solution.version, "solution.version")?;
    requirement(&blueprint.engine_version, "engine_version")?;
    if blueprint.components.is_empty() || blueprint.components.len() > MAX_COMPONENTS {
        return Err(SolutionError::new(
            "component_count",
            "components",
            "Expected 1-256 components",
        ));
    }
    for (id, component) in &blueprint.components {
        let path = format!("components.{id}");
        identifier(id, &path)?;
        identifier(
            &component.artifact.pack_id,
            &format!("{path}.artifact.pack_id"),
        )?;
        version(
            &component.artifact.version,
            &format!("{path}.artifact.version"),
        )?;
        digest(
            &component.artifact.sha256,
            &format!("{path}.artifact.sha256"),
        )?;
        let entry = &component.artifact.path;
        if entry.len() > 240
            || entry
                .split('/')
                .any(|p| p.is_empty() || p == "." || p == "..")
            || !entry
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-/".contains(&b))
        {
            return Err(SolutionError::new(
                "invalid_artifact_path",
                format!("{path}.artifact.path"),
                "Expected a portable relative pack entry; no traversal, URL or host path",
            ));
        }
        for (dependency, compatible) in &component.depends_on {
            let dep_path = format!("{path}.depends_on.{dependency}");
            let target = blueprint.components.get(dependency).ok_or_else(|| {
                SolutionError::new("missing_component", &dep_path, "Dependency is not declared")
            })?;
            if !requirement(compatible, &dep_path)?
                .matches(&version(&target.artifact.version, &dep_path)?)
            {
                return Err(SolutionError::new(
                    "incompatible_component",
                    dep_path,
                    "Pinned dependency version does not meet the requirement",
                ));
            }
        }
        for conflict in &component.conflicts_with {
            if conflict == id || !blueprint.components.contains_key(conflict) {
                return Err(SolutionError::new(
                    "invalid_conflict",
                    format!("{path}.conflicts_with.{conflict}"),
                    "Conflict must reference a different declared component",
                ));
            }
        }
        for name in component
            .required_capabilities
            .iter()
            .chain(&component.optional_capabilities)
            .chain(&component.model_classes)
        {
            identifier(name, &path)?;
        }
    }
    topological_order(
        &blueprint.components,
        &blueprint.components.keys().cloned().collect(),
    )?;
    for name in blueprint
        .memory_spaces
        .keys()
        .chain(blueprint.preferences.keys())
        .chain(&blueprint.ui_features)
        .chain(&blueprint.deployment_requirements)
        .chain(&blueprint.constraints.allowed_providers)
    {
        identifier(name, name)?;
    }
    for (name, preference) in &blueprint.preferences {
        check_preference(name, preference, &default_preference(preference), false)?;
    }
    Ok(())
}

pub(crate) fn default_preference(preference: &Preference) -> PreferenceValue {
    match preference {
        Preference::Boolean { default, .. } => PreferenceValue::Boolean(*default),
        Preference::Integer { default, .. } => PreferenceValue::Integer(*default),
        Preference::Choice { default, .. } => PreferenceValue::Choice(default.clone()),
    }
}

pub(crate) fn check_preference(
    name: &str,
    preference: &Preference,
    value: &PreferenceValue,
    is_override: bool,
) -> Result<(), SolutionError> {
    let (allowed, valid) = match (preference, value) {
        (Preference::Boolean { overridable, .. }, PreferenceValue::Boolean(_)) => {
            (*overridable, true)
        }
        (
            Preference::Integer {
                min,
                max,
                overridable,
                ..
            },
            PreferenceValue::Integer(value),
        ) => (*overridable, min <= value && value <= max),
        (
            Preference::Choice {
                choices,
                overridable,
                ..
            },
            PreferenceValue::Choice(value),
        ) => (
            *overridable,
            choices.contains(value)
                && choices.iter().all(|choice| {
                    !choice.is_empty()
                        && choice.len() <= 80
                        && choice.chars().all(|c| !c.is_control())
                }),
        ),
        _ => (false, false),
    };
    if !valid {
        return Err(SolutionError::new(
            "invalid_preference",
            format!("preferences.{name}"),
            "Value must match the declared type and bounds or choices",
        ));
    }
    if is_override && !allowed {
        return Err(SolutionError::new(
            "override_denied",
            format!("preferences.{name}"),
            "This preference is not customer-overridable",
        ));
    }
    Ok(())
}

pub(crate) fn topological_order(
    components: &BTreeMap<String, Component>,
    selected: &BTreeSet<String>,
) -> Result<Vec<String>, SolutionError> {
    let mut remaining = selected.clone();
    let mut done = BTreeSet::new();
    let mut order = Vec::new();
    while !remaining.is_empty() {
        let next = remaining
            .iter()
            .find(|id| {
                components[*id]
                    .depends_on
                    .keys()
                    .all(|dep| done.contains(dep))
            })
            .cloned();
        let Some(next) = next else {
            return Err(SolutionError::new(
                "dependency_cycle",
                format!(
                    "components.{}",
                    remaining.first().map(String::as_str).unwrap_or("")
                ),
                "Dependency graph contains a cycle",
            ));
        };
        remaining.remove(&next);
        done.insert(next.clone());
        order.push(next);
    }
    Ok(order)
}
