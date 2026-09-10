//! PFSerialize CLI: hex dump / demo roundtrip.

use std::env;
use std::process;

use pf_core::test_utils::TestHost;
use pf_core::value::normalize_for_cmp;
use pf_core::{decode, encode, encode_with, EncodeOptions, Value};
use pf_format::{FLAG_CHECKSUM, FLAG_STRDEDUP};

fn main() {
    let mut args = env::args().skip(1);
    let cmd = args.next().unwrap_or_else(|| "help".into());
    match cmd.as_str() {
        "demo" => cmd_demo(),
        "hex" => {
            let rest: Vec<String> = args.collect();
            if rest.is_empty() {
                eprintln!("usage: pfserialize hex <hex-bytes>");
                process::exit(2);
            }
            cmd_hex(&rest.join(""));
        }
        "help" | "-h" | "--help" => print_help(),
        other => {
            eprintln!("unknown command: {other}");
            print_help();
            process::exit(2);
        }
    }
}

fn print_help() {
    eprintln!(
        "\
PFSerialize CLI

Usage:
  pfserialize demo          Encode sample payloads and print hex + roundtrip
  pfserialize hex <hex>     Decode a PFBinary hex string and print debug value
  pfserialize help
"
    );
}

fn cmd_demo() {
    let samples: Vec<(&str, Value)> = vec![
        (
            "packed [1,2,3,4]",
            Value::ArrayPacked(vec![
                Value::Int(1),
                Value::Int(2),
                Value::Int(3),
                Value::Int(4),
            ]),
        ),
        (
            "assoc user",
            Value::map_str(vec![
                ("id", Value::Int(10001)),
                ("name", Value::str("Tom")),
                ("price", Value::Float(99.99)),
                ("active", Value::Bool(true)),
                ("extra", Value::Null),
            ]),
        ),
        (
            "repeated status",
            Value::ArrayPacked(vec![
                Value::map_str(vec![
                    ("id", Value::Int(1)),
                    ("status", Value::str("active")),
                ]),
                Value::map_str(vec![
                    ("id", Value::Int(2)),
                    ("status", Value::str("active")),
                ]),
                Value::map_str(vec![
                    ("id", Value::Int(3)),
                    ("status", Value::str("active")),
                ]),
            ]),
        ),
    ];

    for (name, v) in samples {
        let bytes = encode(&v).expect("encode");
        println!("== {name} ==");
        println!("hex: {}", to_hex(&bytes));
        println!("len: {}", bytes.len());
        let mut host = TestHost;
        let out = decode(&bytes, &mut host).expect("decode");
        let ok = normalize_for_cmp(&out) == normalize_for_cmp(&v);
        println!("roundtrip_ok: {ok}");

        let with_cs = encode_with(
            &v,
            &EncodeOptions {
                flags: FLAG_CHECKSUM | FLAG_STRDEDUP,
            },
        )
        .unwrap();
        println!("with_checksum_len: {}", with_cs.len());
        println!();
    }
}

fn cmd_hex(hex: &str) {
    let bytes = parse_hex(hex).unwrap_or_else(|e| {
        eprintln!("invalid hex: {e}");
        process::exit(1);
    });
    let mut host = TestHost;
    match decode(&bytes, &mut host) {
        Ok(v) => {
            println!("ok: {v:?}");
            println!("len: {}", bytes.len());
        }
        Err(e) => {
            eprintln!("decode error: {e}");
            process::exit(1);
        }
    }
}

fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn parse_hex(s: &str) -> Result<Vec<u8>, String> {
    let clean: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if clean.len() % 2 != 0 {
        return Err("odd length".into());
    }
    let mut out = Vec::with_capacity(clean.len() / 2);
    let bytes = clean.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let hi = from_hex(bytes[i])?;
        let lo = from_hex(bytes[i + 1])?;
        out.push((hi << 4) | lo);
        i += 2;
    }
    Ok(out)
}

fn from_hex(c: u8) -> Result<u8, String> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        b'A'..=b'F' => Ok(c - b'A' + 10),
        _ => Err(format!("bad hex digit {}", c as char)),
    }
}
