#![feature(conservative_impl_trait)]
#![feature(rustc_diagnostic_macros)]
#![feature(rustc_private)]

#[macro_use]
extern crate log;

#[cfg(test)]
#[macro_use]
extern crate quickcheck;

extern crate rustc;
extern crate rustc_data_structures;
extern crate semver;
extern crate syntax;
extern crate syntax_pos;

pub mod semcheck;
