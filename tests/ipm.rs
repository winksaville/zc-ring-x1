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
