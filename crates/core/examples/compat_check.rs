//! Compares this build's persisted shape against a file the C# app wrote.
//!
//! Usage: `cargo run -p syncmaid-core --example compat_check -- <tasks.json>`
//!
//! It loads the file, writes it back out, and reports how the two differ — without printing
//! the contents, which are the user's own paths.

use std::path::PathBuf;

use syncmaid_core::model::SyncTask;

fn main() {
    let Some(path) = std::env::args().nth(1).map(PathBuf::from) else {
        eprintln!("usage: compat_check <tasks.json>");
        std::process::exit(2);
    };

    let original = std::fs::read_to_string(&path).expect("read the file");
    let tasks: Vec<SyncTask> = match serde_json::from_str(&original) {
        Ok(tasks) => tasks,
        Err(error) => {
            println!("LOAD FAILED: {error}");
            std::process::exit(1);
        }
    };

    println!("loaded {} task(s)", tasks.len());
    for task in &tasks {
        println!(
            "  kind {:?} ({}), trigger {}, {} destination(s)",
            task.kind(),
            if task.has_explicit_kind() {
                "explicit"
            } else {
                "derived from destinations"
            },
            trigger_name(&task.trigger),
            task.destinations.len()
        );
    }

    let rewritten = syncmaid_core::persistence::to_config_json(&tasks).expect("serialize");

    let same_bytes = original == rewritten;
    let same_ignoring_newlines = original.replace("\r\n", "\n") == rewritten.replace("\r\n", "\n");

    println!("byte-identical: {same_bytes}");
    println!("identical ignoring line endings: {same_ignoring_newlines}");

    if !same_ignoring_newlines {
        // Report *where* the shapes diverge without echoing the values.
        let mut left = original.replace("\r\n", "\n");
        let mut right = rewritten.replace("\r\n", "\n");
        // Legacy files gain a Kind on the way out; that is the intended normalization.
        left.retain(|c| c != ' ');
        right.retain(|c| c != ' ');
        let divergence = left
            .lines()
            .zip(right.lines())
            .enumerate()
            .find(|(_, (a, b))| a != b)
            .map(|(line, _)| line + 1);
        match divergence {
            Some(line) => println!("first structural difference at line {line}"),
            None => println!("one side simply has more lines than the other"),
        }
        std::process::exit(1);
    }
}

fn trigger_name(trigger: &syncmaid_core::triggers::Trigger) -> &'static str {
    use syncmaid_core::triggers::Trigger;
    match trigger {
        Trigger::Manual => "manual",
        Trigger::Scheduled { .. } => "scheduled",
        Trigger::Watch { .. } => "watch",
    }
}
