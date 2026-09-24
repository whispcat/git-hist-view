//! Native harness for the engine: verifies git plumbing against real repositories and benchmarks each view.

mod bench;
mod git;

use std::{env, path::Path, process::ExitCode};

type CliResult = Result<(), Box<dyn std::error::Error>>;

const USAGE: &str = "\
usage: ghv <command> <args>

  walk <repo>             first-parent history walk
  stats <repo>            full analysis: history, churn, coupling
  treemap <repo>          treemap frames at every keyframe, both layouts
  coupling <repo>         coupling graph layouts at every keyframe
  deps <repo>             parse and resolve imports at HEAD
  pack <file.pack>        index a pack and check it against git's .idx
  fetch <url> [depth]     fetch over git protocol v2 like the browser does";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["walk", repo] => git::walk(Path::new(repo)),
        ["stats", repo] => bench::stats(Path::new(repo)),
        ["treemap", repo] => bench::treemap(Path::new(repo)),
        ["coupling", repo] => bench::coupling(Path::new(repo)),
        ["deps", repo] => bench::deps(Path::new(repo)),
        ["pack", file] => git::verify_pack(Path::new(file)),
        ["fetch", url] => git::fetch(url, None),
        ["fetch", url, depth] => depth.parse().map_err(Into::into).and_then(|d| git::fetch(url, Some(d))),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
