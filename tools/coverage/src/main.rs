//! `tools-coverage` — aggregate and summarize `lcov` coverage reports.
//!
//! Paired with `tools/coverage/coverage.sh`, which runs `bazel coverage //...`
//! and feeds the produced `coverage/lcov.info` to `summarize`.

#![allow(clippy::cast_precision_loss)]

use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "orbit-coverage", about = "Summarize lcov coverage reports")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(clap::Subcommand)]
enum Cmd {
    /// Parse an lcov file and print a coverage summary.
    Summarize {
        /// Path to lcov.info.
        path: PathBuf,
        /// Output format.
        #[arg(long, value_enum, default_value_t = Out::Text)]
        format: Out,
    },
}

#[derive(Copy, Clone, ValueEnum)]
enum Out {
    Text,
    Json,
}

#[derive(Serialize)]
struct FileCov {
    file: String,
    found: u64,
    hit: u64,
    percent: f64,
}

#[derive(Serialize)]
struct Summary {
    files: Vec<FileCov>,
    total_found: u64,
    total_hit: u64,
    total_percent: f64,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Summarize { path, format } => {
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            let mut files: Vec<FileCov> = Vec::new();
            let mut cur_file: Option<String> = None;
            let mut found = 0u64;
            let mut hit = 0u64;
            let mut tf = 0u64;
            let mut th = 0u64;

            for line in text.lines() {
                if let Some(rest) = line.strip_prefix("SF:") {
                    cur_file = Some(rest.to_string());
                    found = 0;
                    hit = 0;
                } else if let Some(rest) = line.strip_prefix("LF:") {
                    found = rest.trim().parse().unwrap_or(0);
                } else if let Some(rest) = line.strip_prefix("LH:") {
                    hit = rest.trim().parse().unwrap_or(0);
                } else if line == "end_of_record" {
                    if let Some(file) = cur_file.take() {
                        tf += found;
                        th += hit;
                        let percent = if found > 0 {
                            (hit as f64 / found as f64) * 100.0
                        } else {
                            100.0
                        };
                        files.push(FileCov {
                            file,
                            found,
                            hit,
                            percent,
                        });
                    }
                }
            }

            let total_percent = if tf > 0 {
                (th as f64 / tf as f64) * 100.0
            } else {
                100.0
            };
            let summary = Summary {
                files,
                total_found: tf,
                total_hit: th,
                total_percent,
            };

            match format {
                Out::Json => {
                    println!("{}", serde_json::to_string_pretty(&summary)?);
                }
                Out::Text => {
                    for f in &summary.files {
                        println!("{:6.2}%  {}/{}  {}", f.percent, f.hit, f.found, f.file);
                    }
                    println!("---");
                    println!(
                        "TOTAL {:.2}%  {}/{}",
                        summary.total_percent, summary.total_hit, summary.total_found
                    );
                }
            }
        }
    }
    Ok(())
}
