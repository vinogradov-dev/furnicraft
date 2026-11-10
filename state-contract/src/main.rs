//! The `state-contract` command line: the plan tool.
//!
//! One binary, one table: `state-contract help` lists every command. A project that embeds the tool
//! in its own runner calls [`state_contract::cli::main_with`] and registers its own commands there.
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

fn main() {
    std::process::exit(state_contract::cli::main());
}
