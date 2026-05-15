/// Eval Runner CLI Binary
///
/// Command-line tool for executing AI evaluation datasets.
///
/// Usage:
///   cargo run --bin eval-runner -- --dataset eval_datasets/critical_path.yaml --output /tmp/results.json
///   cargo run --bin eval-runner -- --dataset eval_datasets/critical_path.yaml --simulation
///   cargo run --bin eval-runner -- --help
use std::path::PathBuf;
use std::process::ExitCode;

use tandem_server::eval::runner::EngineMode;
use tandem_server::eval::{EvalRunner, EvalRunnerConfig};

const USAGE: &str = r#"
Tandem Eval Runner - AI Quality Evaluation Tool

USAGE:
    eval-runner [OPTIONS]

OPTIONS:
    --dataset <FILE>          Path to eval dataset YAML file (required)
    --output <FILE>           Output path for results JSON [default: ./eval_results.json]
    --provider <NAME>         AI provider to use [default: anthropic]
    --model <NAME>            Model to use [default: claude-haiku-4-5-20251001]
    --engine-mode <MODE>      Execution mode: simulation | stub | live [default: simulation]
                                simulation: hardcoded deterministic outcomes, no engine
                                stub:       real engine + scripted stub provider (no API)
                                live:       real engine + real provider (needs API key)
    --simulation              Legacy alias for --engine-mode simulation
    --num-workers <N>         Parallel workers [default: 1]
    --filter-tag <TAG>        Only run tests with this tag
    --max-duration <SECS>     Max time per test in seconds [default: 300]
    --verbose                 Print detailed output during execution
    --help                    Print this help message

EXAMPLES:
    # Run critical path tests in simulation mode (default)
    eval-runner --dataset eval_datasets/critical_path.yaml

    # Run against the scripted engine stub
    eval-runner --dataset eval_datasets/critical_path.yaml --engine-mode stub

    # Run only tests tagged as "regression"
    eval-runner --dataset eval_datasets/critical_path.yaml \
                --filter-tag regression --engine-mode simulation

EXIT CODES:
    0    All tests passed
    1    One or more tests failed
    2    Error loading dataset or invalid arguments
"#;

struct CliArgs {
    dataset: PathBuf,
    output: PathBuf,
    provider: String,
    model: String,
    engine_mode: EngineMode,
    num_workers: u32,
    filter_tag: Option<String>,
    max_duration_secs: u64,
    verbose: bool,
}

impl CliArgs {
    fn parse() -> Result<Self, String> {
        let args: Vec<String> = std::env::args().collect();

        let mut dataset: Option<PathBuf> = None;
        let mut output = PathBuf::from("./eval_results.json");
        let mut provider = "anthropic".to_string();
        let mut model = "claude-haiku-4-5-20251001".to_string();
        let mut engine_mode = EngineMode::Simulation;
        let mut num_workers = 1u32;
        let mut filter_tag: Option<String> = None;
        let mut max_duration_secs = 300u64;
        let mut verbose = false;

        let mut i = 1;
        while i < args.len() {
            match args[i].as_str() {
                "--help" | "-h" => {
                    println!("{}", USAGE);
                    std::process::exit(0);
                }
                "--dataset" => {
                    i += 1;
                    if i >= args.len() {
                        return Err("--dataset requires a file path".to_string());
                    }
                    dataset = Some(PathBuf::from(&args[i]));
                }
                "--output" => {
                    i += 1;
                    if i >= args.len() {
                        return Err("--output requires a file path".to_string());
                    }
                    output = PathBuf::from(&args[i]);
                }
                "--provider" => {
                    i += 1;
                    if i >= args.len() {
                        return Err("--provider requires a name".to_string());
                    }
                    provider = args[i].clone();
                }
                "--model" => {
                    i += 1;
                    if i >= args.len() {
                        return Err("--model requires a name".to_string());
                    }
                    model = args[i].clone();
                }
                "--simulation" => {
                    engine_mode = EngineMode::Simulation;
                }
                "--engine-mode" => {
                    i += 1;
                    if i >= args.len() {
                        return Err("--engine-mode requires a value".to_string());
                    }
                    engine_mode = EngineMode::parse(&args[i])?;
                }
                "--num-workers" => {
                    i += 1;
                    if i >= args.len() {
                        return Err("--num-workers requires a number".to_string());
                    }
                    num_workers = args[i]
                        .parse()
                        .map_err(|_| "--num-workers must be a number".to_string())?;
                }
                "--filter-tag" => {
                    i += 1;
                    if i >= args.len() {
                        return Err("--filter-tag requires a tag name".to_string());
                    }
                    filter_tag = Some(args[i].clone());
                }
                "--max-duration" => {
                    i += 1;
                    if i >= args.len() {
                        return Err("--max-duration requires seconds".to_string());
                    }
                    max_duration_secs = args[i]
                        .parse()
                        .map_err(|_| "--max-duration must be a number".to_string())?;
                }
                "--verbose" | "-v" => {
                    verbose = true;
                }
                unknown => {
                    return Err(format!("Unknown argument: {}", unknown));
                }
            }
            i += 1;
        }

        let dataset = dataset.ok_or_else(|| "--dataset is required".to_string())?;

        Ok(Self {
            dataset,
            output,
            provider,
            model,
            engine_mode,
            num_workers,
            filter_tag,
            max_duration_secs,
            verbose,
        })
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let args = match CliArgs::parse() {
        Ok(args) => args,
        Err(e) => {
            eprintln!("Error: {}\n", e);
            eprintln!("{}", USAGE);
            return ExitCode::from(2);
        }
    };

    println!("Tandem Eval Runner v0.1.0");
    println!("Dataset: {}", args.dataset.display());
    println!("Output: {}", args.output.display());
    match args.engine_mode {
        EngineMode::Simulation => println!("Mode: SIMULATION (no AI calls, deterministic)"),
        EngineMode::Stub => {
            println!("Mode: STUB (scripted engine, no API key needed)");
        }
        EngineMode::Live => println!("Mode: LIVE ({}/{})", args.provider, args.model),
    }
    if let Some(tag) = &args.filter_tag {
        println!("Filter Tag: {}", tag);
    }
    println!();

    let simulation_mode = matches!(args.engine_mode, EngineMode::Simulation);
    let config = EvalRunnerConfig {
        num_workers: args.num_workers,
        default_provider: args.provider,
        default_model: args.model,
        max_test_duration_secs: args.max_duration_secs,
        engine_mode: args.engine_mode,
        simulation_mode,
        random_seed: None,
    };

    let runner = EvalRunner::new(config);

    let dataset = match runner.load_dataset(&args.dataset) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Failed to load dataset: {}", e);
            return ExitCode::from(2);
        }
    };

    println!(
        "Loaded dataset '{}' v{} ({} test cases)",
        dataset.name,
        dataset.version,
        dataset.test_cases.len()
    );

    // Apply tag filter if specified
    let filtered_dataset = if let Some(tag) = &args.filter_tag {
        let mut filtered = dataset.clone();
        filtered.test_cases = filtered
            .test_cases
            .into_iter()
            .filter(|tc| tc.tags.contains(tag))
            .collect();
        println!(
            "Filtered to {} test cases with tag '{}'",
            filtered.test_cases.len(),
            tag
        );
        filtered
    } else {
        dataset
    };

    if args.verbose {
        println!("\nTest cases to run:");
        for tc in &filtered_dataset.test_cases {
            println!(
                "  [{}] {} - {}",
                if tc.enabled { "✓" } else { "○" },
                tc.id,
                tc.description
            );
        }
        println!();
    }

    println!("Running evaluation...\n");
    let metrics = runner.run_dataset(&filtered_dataset).await;

    // Print summary
    println!("{}", metrics.summary());

    // Save results
    if let Err(e) = runner.save_results(&metrics, &args.output) {
        eprintln!("Warning: Failed to save results: {}", e);
    } else {
        println!("\nResults saved to: {}", args.output.display());
    }

    // Exit with appropriate code
    if metrics.failed_tests > 0 {
        eprintln!(
            "\n❌ {} of {} tests failed",
            metrics.failed_tests, metrics.total_tests
        );
        ExitCode::from(1)
    } else {
        println!("\n✅ All {} tests passed", metrics.passed_tests);
        ExitCode::SUCCESS
    }
}
