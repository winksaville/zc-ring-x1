//! The inter-process message: `zcr-test-ipm consumer` and
//! `zcr-test-ipm producer` as two processes, the consumer first,
//! and the value the producer sent is the value the consumer
//! received with its checksum verified.
//!
//! - Twenty rounds in one test, since the two share one
//!   hard-coded region file and must not run beside another pair.
//! - Each round prints the producer's and the consumer's lines,
//!   which `-- --show-output` displays.

#![cfg(target_os = "linux")]

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

/// The app, under its cycle name or its landed one.
fn app() -> &'static str {
    option_env!("CARGO_BIN_EXE_zcr-test-ipm")
        .or(option_env!("CARGO_BIN_EXE_zcr-test-ipm-dev"))
        .expect("the zcr-test-ipm binary is built for integration tests")
}

/// The value field of a `sent` or `received` line.
fn value(line: &str) -> &str {
    line.split_whitespace()
        .find(|w| w.starts_with("value="))
        .unwrap_or_else(|| panic!("no value in {line:?}"))
}

#[test]
fn one_message_crosses_between_processes() {
    for round in 0..20 {
        let mut consumer = Command::new(app())
            .arg("consumer")
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut lines = BufReader::new(consumer.stdout.take().unwrap()).lines();
        assert_eq!(lines.next().unwrap().unwrap(), "ready", "round {round}");

        let producer = Command::new(app()).arg("producer").output().unwrap();
        assert!(producer.status.success(), "round {round}: {producer:?}");
        let sent = String::from_utf8(producer.stdout).unwrap();

        let received = lines.next().unwrap().unwrap();
        assert!(consumer.wait().unwrap().success(), "round {round}");
        assert!(sent.starts_with("sent "), "round {round}: {sent:?}");
        assert!(received.ends_with(" ok"), "round {round}: {received:?}");
        assert_eq!(value(&sent), value(&received), "round {round}");
        // Shown by `cargo test --test ipm -- --show-output`.
        println!("round {round:2}: {} | {received}", sent.trim_end());
    }
}

/// Spawn the app with `args`, its stdout piped.
fn spawn(args: &[&str]) -> std::process::Child {
    Command::new(app())
        .args(args)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap()
}

/// Wait for a child and return its stdout, asserting it succeeded.
fn finish(child: std::process::Child, what: &str) -> String {
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{what}: {out:?}");
    String::from_utf8(out.stdout).unwrap()
}

/// Spawn an MPSC consumer and wait for its `ready` line, returning
/// the child and the rest of its output to come.
fn mpsc_consumer(how: &str, messages: u64) -> std::process::Child {
    let mut child = spawn(&["mpsc-consumer", how, &messages.to_string()]);
    let mut ready = String::new();
    BufReader::new(child.stdout.as_mut().unwrap())
        .read_line(&mut ready)
        .unwrap();
    assert_eq!(ready.trim_end(), "ready", "{how} consumer");
    child
}

/// Each producer's first and last number from a consumer's output.
fn ranges(out: &str) -> std::collections::BTreeMap<u64, (u64, u64)> {
    out.lines()
        .filter(|l| l.starts_with("received producer="))
        .map(|l| {
            let field = |name: &str| -> u64 {
                l.split_whitespace()
                    .find_map(|w| w.strip_prefix(name))
                    .unwrap()
                    .parse()
                    .unwrap()
            };
            (field("producer="), (field("first="), field("last=")))
        })
        .collect()
}

#[test]
fn mpsc_producers_and_consumers_come_and_go_between_processes() {
    const EACH: u64 = 20_000;
    // A consumer makes the ring, and a release while it holds the
    // consumer role is refused.
    let first = mpsc_consumer("new", EACH);
    let refused = Command::new(app()).arg("mpsc-release").output().unwrap();
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("RingInUse"));

    // Two producer processes at once, and the first consumer reads
    // half of what they send, releases, and exits, while they wait
    // on the full ring for the next.
    let p0 = spawn(&["mpsc-producer", "0", &EACH.to_string()]);
    let p1 = spawn(&["mpsc-producer", "1", &EACH.to_string()]);
    let first = finish(first, "first consumer");
    let second = mpsc_consumer("join", EACH);
    finish(p0, "producer 0");
    finish(p1, "producer 1");
    let second = finish(second, "second consumer");

    // The second consumer continued each producer's stream exactly
    // where the first stopped, and together they read everything.
    let (a, b) = (ranges(&first), ranges(&second));
    for p in 0..2u64 {
        let (a_first, a_last) = a.get(&p).copied().unwrap_or((0, u64::MAX));
        let (b_first, b_last) = b[&p];
        assert_eq!(a_first, 0, "producer {p}");
        assert_eq!(b_first, a_last.wrapping_add(1), "producer {p}");
        assert_eq!(b_last, EACH - 1, "producer {p}");
    }

    // A producer that comes after the others have gone, to a third
    // consumer, and then the ring, no role held, is released, and
    // a producer can no longer join it.
    let third = mpsc_consumer("join", 100);
    finish(spawn(&["mpsc-producer", "2", "100"]), "producer 2");
    let third = finish(third, "third consumer");
    assert_eq!(ranges(&third)[&2], (0, 99));
    finish(spawn(&["mpsc-release"]), "release");
    let late = Command::new(app())
        .args(["mpsc-producer", "3", "1"])
        .output()
        .unwrap();
    assert!(!late.status.success());
    assert!(String::from_utf8_lossy(&late.stderr).contains("BadMagic"));
}
