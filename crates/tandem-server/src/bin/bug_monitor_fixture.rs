use std::path::PathBuf;
use std::process::ExitCode;

use tandem_server::bug_monitor::regression_fixture::{
    write_incident_regression_fixture, BugMonitorRegressionFixtureOptions,
};

const USAGE: &str = r#"
Bug Monitor Regression Fixture Scaffold

USAGE:
    bug-monitor-fixture --incident <FILE> --output <FILE> [OPTIONS]

OPTIONS:
    --incident <FILE>    Bug Monitor incident JSON export
    --output <FILE>      Output YAML dataset path
    --id <ID>            Override generated test case id
    --tag <TAG>          Add an extra test tag; may be repeated
    --dataset-name <N>   Override generated dataset name
    --help               Print this help message

EXAMPLE:
    cargo run -p tandem-server --bin bug-monitor-fixture -- \
      --incident /tmp/incident.json \
      --output eval_datasets/regressions/dogfood_001.yaml \
      --id dogfood_001_provider_timeout
"#;

#[derive(Debug)]
struct CliArgs {
    incident: PathBuf,
    output: PathBuf,
    fixture_id: Option<String>,
    dataset_name: Option<String>,
    extra_tags: Vec<String>,
}

impl CliArgs {
    fn parse() -> Result<Self, String> {
        let args = std::env::args().skip(1).collect::<Vec<_>>();
        let mut incident = None;
        let mut output = None;
        let mut fixture_id = None;
        let mut dataset_name = None;
        let mut extra_tags = Vec::new();

        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "--help" | "-h" => {
                    println!("{USAGE}");
                    std::process::exit(0);
                }
                "--incident" => {
                    i += 1;
                    incident = Some(PathBuf::from(required_arg(&args, i, "--incident")?));
                }
                "--output" => {
                    i += 1;
                    output = Some(PathBuf::from(required_arg(&args, i, "--output")?));
                }
                "--id" => {
                    i += 1;
                    fixture_id = Some(required_arg(&args, i, "--id")?.to_string());
                }
                "--dataset-name" => {
                    i += 1;
                    dataset_name = Some(required_arg(&args, i, "--dataset-name")?.to_string());
                }
                "--tag" => {
                    i += 1;
                    extra_tags.push(required_arg(&args, i, "--tag")?.to_string());
                }
                unknown => return Err(format!("unknown argument: {unknown}")),
            }
            i += 1;
        }

        Ok(Self {
            incident: incident.ok_or_else(|| "--incident is required".to_string())?,
            output: output.ok_or_else(|| "--output is required".to_string())?,
            fixture_id,
            dataset_name,
            extra_tags,
        })
    }
}

fn required_arg<'a>(args: &'a [String], index: usize, flag: &str) -> Result<&'a str, String> {
    args.get(index)
        .map(String::as_str)
        .filter(|value| !value.starts_with("--"))
        .ok_or_else(|| format!("{flag} requires a value"))
}

fn main() -> ExitCode {
    let args = match CliArgs::parse() {
        Ok(args) => args,
        Err(error) => {
            eprintln!("Error: {error}\n{USAGE}");
            return ExitCode::from(2);
        }
    };

    let options = BugMonitorRegressionFixtureOptions {
        fixture_id: args.fixture_id,
        dataset_name: args.dataset_name,
        extra_tags: args.extra_tags,
    };

    match write_incident_regression_fixture(&args.incident, &args.output, options) {
        Ok(()) => {
            println!(
                "Wrote dogfooding regression fixture to {}",
                args.output.display()
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("Failed to write dogfooding regression fixture: {error:#}");
            ExitCode::from(1)
        }
    }
}
