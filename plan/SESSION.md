# Session

```text
task: U80
goal: cargo xtask ui-rules fails a literal colour and a raw egui host outside the module that owns it

steps:
  [ ] commit the rule, the owner tables and the colour fixes

now: commit the rule, the owner tables and the colour fixes
blocked: nothing
run: cargo xtask U80
resume: the rule is green and cargo xtask ci passed; commit, then close U80
```
