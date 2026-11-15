//! `chanorbit` command line: JSON on stdin, JSON on stdout. See docs/cli-contract.md.
use chanorbit::contract::{Command, SCHEMA, STAGES_SCHEMA, handle_bytes};
use std::io::{Read, Write};
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};

const USAGE: &str = "usage: chanorbit geometry|stages [--batch [--jobs N]]\n       chanorbit --version\n       chanorbit --help";

/// All responses written and successful.
const EXIT_OK: u8 = 0;
/// All responses written; at least one is an error object.
const EXIT_REQUEST_ERROR: u8 = 1;
/// Bad command line; nothing written to stdout.
const EXIT_USAGE: u8 = 2;
/// stdin unreadable or stdout unwritable.
const EXIT_IO: u8 = 3;

struct Args {
    command: Command,
    batch: bool,
    jobs: Option<usize>,
}

fn parse_args(args: &[String]) -> Result<Option<Args>, String> {
    match args.first().map(String::as_str) {
        Some("--version" | "-V") if args.len() == 1 => {
            println!(
                "chanorbit {} schemas {SCHEMA} {STAGES_SCHEMA}",
                env!("CARGO_PKG_VERSION")
            );
            Ok(None)
        }
        Some("--help" | "-h") if args.len() == 1 => {
            println!("{USAGE}");
            Ok(None)
        }
        Some(name @ ("geometry" | "stages")) => {
            let mut parsed = Args {
                command: if name == "stages" {
                    Command::Stages
                } else {
                    Command::Geometry
                },
                batch: false,
                jobs: None,
            };
            let mut rest = args[1..].iter();
            while let Some(arg) = rest.next() {
                match arg.as_str() {
                    "--batch" => parsed.batch = true,
                    "--jobs" => {
                        let value = rest.next().ok_or("--jobs needs a value")?;
                        parsed.jobs =
                            Some(value.parse().ok().filter(|&n| n > 0).ok_or_else(|| {
                                format!("--jobs: not a positive integer: {value}")
                            })?);
                    }
                    other => return Err(format!("unknown argument: {other}")),
                }
            }
            if parsed.jobs.is_some() && !parsed.batch {
                return Err("--jobs needs --batch".into());
            }
            Ok(Some(parsed))
        }
        Some(other) => Err(format!("unknown command: {other}")),
        None => Err("missing command".into()),
    }
}

/// Handle lines in parallel; results keep input order.
fn handle_lines(command: Command, lines: &[&[u8]], jobs: usize) -> Vec<(String, bool)> {
    let next = AtomicUsize::new(0);
    let mut results: Vec<Option<(String, bool)>> = vec![None; lines.len()];
    let workers = jobs.min(lines.len()).max(1);
    let chunks: Vec<Vec<(usize, (String, bool))>> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(line) = lines.get(i) else { break };
                        done.push((i, handle_bytes(command, line)));
                    }
                    done
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("worker panicked"))
            .collect()
    });
    for (i, result) in chunks.into_iter().flatten() {
        results[i] = Some(result);
    }
    results
        .into_iter()
        .map(|r| r.expect("every line handled"))
        .collect()
}

fn run(args: Args) -> Result<bool, String> {
    let mut input = Vec::new();
    std::io::stdin()
        .lock()
        .read_to_end(&mut input)
        .map_err(|e| format!("reading stdin: {e}"))?;
    let results = if args.batch {
        // Lines are split on bytes, so one line that is not UTF-8 fails alone.
        let lines: Vec<&[u8]> = input
            .split(|&b| b == b'\n')
            .map(|l| l.strip_suffix(b"\r").unwrap_or(l))
            .filter(|l| !l.iter().all(u8::is_ascii_whitespace))
            .collect();
        let jobs = args
            .jobs
            .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get()));
        handle_lines(args.command, &lines, jobs)
    } else {
        vec![handle_bytes(args.command, &input)]
    };
    let mut out = std::io::BufWriter::new(std::io::stdout().lock());
    let mut ok = true;
    for (line, success) in &results {
        ok &= success;
        writeln!(out, "{line}").map_err(|e| format!("writing stdout: {e}"))?;
    }
    out.flush().map_err(|e| format!("writing stdout: {e}"))?;
    Ok(ok)
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&argv) {
        Ok(Some(args)) => args,
        Ok(None) => return ExitCode::from(EXIT_OK),
        Err(message) => {
            eprintln!("chanorbit: {message}\n{USAGE}");
            return ExitCode::from(EXIT_USAGE);
        }
    };
    match run(args) {
        Ok(true) => ExitCode::from(EXIT_OK),
        Ok(false) => ExitCode::from(EXIT_REQUEST_ERROR),
        Err(message) => {
            eprintln!("chanorbit: {message}");
            ExitCode::from(EXIT_IO)
        }
    }
}
