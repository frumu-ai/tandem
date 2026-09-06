fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "{}",
        serde_json::to_string_pretty(&tandem_solutions::model_profile_schema())?
    );
    Ok(())
}
