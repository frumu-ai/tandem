// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1
//
//! Mission / plan compiler boundary for Tandem.
//!
//! This crate is being extracted so the compiler can live behind a distinct
//! distribution and licensing boundary while depending on shared permissive
//! schema crates.
//!
//! Consumers should prefer `tandem_plan_compiler::api` as the supported public
//! surface. Internal modules are intentionally kept private unless they are
//! explicitly reexported through that API boundary.

#![allow(warnings)]

pub mod api;
mod automation_projection;
mod contracts;
mod decomposition;
mod dependency_planner;
mod host;
mod materialization;
mod mission_blueprint;
mod mission_preview;
mod mission_runtime;
mod plan_bundle;
mod plan_overlap;
mod plan_package;
mod plan_validation;
mod planner_build;
mod planner_drafts;
mod planner_invoke;
mod planner_loop;
mod planner_messages;
mod planner_prompts;
mod planner_session;
mod planner_types;
mod runtime_projection;
mod workflow_plan;

pub struct MissionCompiler;

impl MissionCompiler {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MissionCompiler {
    fn default() -> Self {
        Self::new()
    }
}
