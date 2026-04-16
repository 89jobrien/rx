#!/usr/bin/env rust-script
//! A tiny demo script for `rx`.

fn main() {
    let mut args = std::env::args().skip(1);
    let mut name = "world".to_string();

    while let Some(arg) = args.next() {
        if arg == "--name" {
            if let Some(value) = args.next() {
                name = value;
            }
        }
    }

    println!("hello, {name}!");
}
