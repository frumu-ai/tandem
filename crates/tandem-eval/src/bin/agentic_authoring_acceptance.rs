use std::path::PathBuf;
use std::process::ExitCode;

use tandem_eval::run_agentic_product_authoring_acceptance;

const USAGE: &str = r#"Agentic Product Authoring Acceptance (TAN-729)

USAGE:
    agentic-authoring-acceptance --dataset <FILE> [--output <FILE>]

OPTIONS:
    --dataset <FILE>    Versioned EvalDataset corpus to execute
    --output <FILE>     JSON report path [default: ./agentic_authoring_results.json]
    --help              Print this help
"#;

struct Args {
    dataset: PathBuf,
    output: PathBuf,
}

impl Args {
    fn parse() -> Result<Self, String> {
        let mut dataset = None;
        let mut output = PathBuf::from("./agentic_authoring_results.json");
        let mut args = std::env::args().skip(1);
        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--dataset" => {
                    dataset = args.next().map(PathBuf::from);
                    if dataset.is_none() {
                        return Err("--dataset requires a file path".to_string());
                    }
                }
                "--output" => {
                    let Some(path) = args.next() else {
                        return Err("--output requires a file path".to_string());
                    };
                    output = PathBuf::from(path);
                }
                "--help" | "-h" => {
                    println!("{USAGE}");
                    std::process::exit(0);
                }
                unknown => return Err(format!("unknown argument: {unknown}")),
            }
        }
        Ok(Self {
            dataset: dataset.ok_or_else(|| "--dataset is required".to_string())?,
            output,
        })
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = match Args::parse() {
        Ok(args) => args,
        Err(error) => {
            eprintln!("Error: {error}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let report = match run_agentic_product_authoring_acceptance(&args.dataset).await {
        Ok(report) => report,
        Err(error) => {
            eprintln!("Agentic authoring acceptance could not run: {error:#}");
            return ExitCode::from(2);
        }
    };
    if let Err(error) = report.save(&args.output) {
        eprintln!("Failed to save {}: {error:#}", args.output.display());
        return ExitCode::from(2);
    }
    println!("{}", report.summary());
    println!("Results: {}", args.output.display());
    if report.gate_passed {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}
