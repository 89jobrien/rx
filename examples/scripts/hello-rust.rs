#!/usr/bin/env rust-script
//! Rust runtime demo for `rx`.

fn main() {
    let name = std::env::args()
        .skip(1)
        .collect::<Vec<_>>()
        .windows(2)
        .find_map(|pair| (pair[0] == "--name").then(|| pair[1].clone()))
        .unwrap_or_else(|| "world".to_string());

    println!("hello from rust, {name}!");
}
