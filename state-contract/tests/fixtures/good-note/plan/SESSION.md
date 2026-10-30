task: T1
goal: the first user-visible result

steps:
  [x] the skeleton
  [ ] the behaviour

now: the parser arm in `src/lib.rs`
blocked: nothing
run: cargo test -p state-contract
resume: finish the parser arm and run `cargo test -p state-contract`
