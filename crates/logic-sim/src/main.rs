//! `cargo run -p logic-sim -- --seed N` replays one seed with its trace;
//! `--sweep A..B` runs many seeds; `--mutation NAME` runs a broken variant.
// A standalone developer CLI: its report is plain stdout.
#![allow(clippy::disallowed_macros)]

use logic_sim::config::Mutation;
use logic_sim::{describe, minimise, run};

fn usage() -> ! {
    eprintln!(
        "usage: logic-sim [--seed N | --sweep A..B] [--mutation NAME] [--no-minimise] [--quiet] [--strict]\n\
         mutations: {}",
        Mutation::ALL.map(Mutation::name).join(", ")
    );
    std::process::exit(2)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut seed: Option<u64> = None;
    let mut sweep: Option<(u64, u64)> = None;
    let mut mutation = None;
    let mut minimise_failures = true;
    let mut quiet = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--seed" => seed = args.next().and_then(|s| s.parse().ok()).or_else(|| usage()),
            "--sweep" => {
                let spec = args.next().unwrap_or_else(|| usage());
                let (a, b) = spec.split_once("..").unwrap_or_else(|| usage());
                sweep = Some((
                    a.parse().unwrap_or_else(|_| usage()),
                    b.parse().unwrap_or_else(|_| usage()),
                ));
            }
            "--mutation" => {
                let name = args.next().unwrap_or_else(|| usage());
                mutation = Some(Mutation::parse(&name).unwrap_or_else(|| usage()));
            }
            "--no-minimise" => minimise_failures = false,
            "--quiet" => quiet = true,
            // Accepted for old command lines: every finding is fatal now.
            "--strict" => {}
            _ => usage(),
        }
    }
    if let Some((a, b)) = sweep {
        let mut failures = 0u64;
        let mut liveness = 0u64;
        let mut acks = 0u64;
        let mut restores = 0u64;
        for seed in a..b {
            let config = logic_sim::config::SimConfig::for_seed(seed).with_mutation(mutation);
            let outcome = run(config);
            acks += outcome.acks;
            restores += outcome.restores_checked;
            if let Some(v) = outcome.safety_violation() {
                failures += 1;
                println!("seed {seed}: {:?}: {}", v.kind, v.detail);
            } else if let Some(v) = outcome.liveness_violation() {
                liveness += 1;
                if !quiet {
                    println!("seed {seed}: (liveness) {}", v.detail);
                }
            }
        }
        println!(
            "swept seeds {a}..{b}: {failures} safety failures, {liveness} liveness stalls, \
             {acks} acknowledged writes, \
             {restores} restores checked"
        );
        std::process::exit(if failures > 0 { 1 } else { 0 });
    }
    let seed = seed.unwrap_or_else(|| usage());
    let mut config = logic_sim::config::SimConfig::for_seed(seed).with_mutation(mutation);
    config.trace = true;
    let outcome = run(config);
    println!("{}", describe(&outcome.config));
    println!("{:?}", outcome.stats);
    match outcome.safety_violation().or(outcome.liveness_violation()) {
        None => {
            println!(
                "ok: {} acknowledged writes, {} restores checked, no violation",
                outcome.acks, outcome.restores_checked
            );
        }
        Some(v) => {
            let mut shown = outcome.config.clone();
            let mut trace = outcome.trace.clone();
            if minimise_failures && v.kind.is_safety() {
                let small = minimise(&outcome.config);
                if describe(&small) != describe(&outcome.config) {
                    println!("minimised: {}", describe(&small));
                    let replay = run(small.clone());
                    trace = replay.trace.clone();
                    shown = small;
                }
            }
            let _ = shown;
            println!("(the seed alone replays this run and its minimisation)");
            println!(
                "VIOLATION {:?} at t={} cell={:?}\n  {}",
                v.kind, v.time, v.cell, v.detail
            );
            if let Some(trace) = trace {
                println!("--- trace (focused on the violating cell) ---\n{trace}");
            }
            std::process::exit(1);
        }
    }
}
