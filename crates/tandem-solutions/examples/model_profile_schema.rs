// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "{}",
        serde_json::to_string_pretty(&tandem_solutions::model_profile_schema())?
    );
    Ok(())
}
