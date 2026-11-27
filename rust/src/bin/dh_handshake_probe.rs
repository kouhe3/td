//! DhHandshake::check_config() on the Rust side: the DH-parameter gate from
//! td/mtproto/DhHandshake.cpp.
//!
//! ```text
//! dh_handshake_probe <g_int> <prime_hex>
//! ```
//!
//! Output: `ok=1` or `message=<text>` (every failure is Status::Error(Slice),
//! whose code is 0).
//!
//! The primality decision has to agree with OpenSSL's BN_is_prime_fasttest_ex:
//! trial division by the first 25 primes, then Miller-Rabin over twelve fixed
//! bases. That is deterministic, so the A/A floor stays exact.

use std::env;
use std::process::ExitCode;

use num_bigint::BigUint;

fn unhex(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 2);
    let mut index = 0;
    while index + 1 < bytes.len() {
        let high = (bytes[index] as char).to_digit(16).unwrap_or(0) as u8;
        let low = (bytes[index + 1] as char).to_digit(16).unwrap_or(0) as u8;
        out.push((high << 4) | low);
        index += 2;
    }
    out
}

fn is_prime(n: &BigUint) -> bool {
    let zero = BigUint::from(0u8);
    let one = BigUint::from(1u8);
    let two = BigUint::from(2u8);
    if n < &two {
        return false;
    }
    if n == &two {
        return true;
    }
    if n % &two == zero {
        return false;
    }
    const SMALL_PRIMES: [u32; 25] = [
        2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47, 53, 59, 61, 67, 71, 73, 79, 83, 89, 97,
    ];
    for small in SMALL_PRIMES {
        let divisor = BigUint::from(small);
        if n == &divisor {
            return true;
        }
        if n % &divisor == zero {
            return false;
        }
    }

    // Miller-Rabin: n - 1 = d * 2^s with d odd.
    let n_minus_one = n - &one;
    let mut d = n_minus_one.clone();
    let mut s = 0u32;
    while &d % &two == zero {
        d /= &two;
        s += 1;
    }
    const BASES: [u32; 12] = [2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37];
    'base: for base in BASES {
        let a = BigUint::from(base);
        if a >= n_minus_one {
            continue;
        }
        let mut x = a.modpow(&d, n);
        if x == one || x == n_minus_one {
            continue;
        }
        for _ in 1..s {
            x = (&x * &x) % n;
            if x == n_minus_one {
                continue 'base;
            }
        }
        return false;
    }
    true
}

fn check_config(g_int: i32, prime_bytes: &[u8]) -> Result<(), String> {
    let prime = BigUint::from_bytes_be(prime_bytes);
    if prime.bits() != 2048 {
        return Err("p is not 2048-bit number".to_string());
    }
    let mod_ok = match g_int {
        2 => &prime % 8u32 == BigUint::from(7u8),
        3 => &prime % 3u32 == BigUint::from(2u8),
        4 => true,
        5 => {
            let r = &prime % 5u32;
            r == BigUint::from(1u8) || r == BigUint::from(4u8)
        }
        6 => {
            let r = &prime % 24u32;
            r == BigUint::from(19u8) || r == BigUint::from(23u8)
        }
        7 => {
            let r = &prime % 7u32;
            r == BigUint::from(3u8) || r == BigUint::from(5u8) || r == BigUint::from(6u8)
        }
        _ => false,
    };
    if !mod_ok {
        return Err("Bad prime mod 4g".to_string());
    }
    if !is_prime(&prime) {
        return Err("p is not a prime number".to_string());
    }
    let half_prime = (&prime - BigUint::from(1u8)) / BigUint::from(2u8);
    if !is_prime(&half_prime) {
        return Err("(p - 1) / 2 is not a prime number".to_string());
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: dh_handshake_probe <g_int> <prime_hex>");
        return ExitCode::from(2);
    }
    let g_int: i32 = args[1].parse().unwrap_or(0);
    let prime = unhex(&args[2]);
    match check_config(g_int, &prime) {
        Ok(()) => println!("ok=1"),
        Err(message) => println!("message={message}"),
    }
    ExitCode::SUCCESS
}