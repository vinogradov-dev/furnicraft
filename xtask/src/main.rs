//! FurniCraft workspace tasks.
//!
//! Usage: `cargo xtask <layers|plan|status|ci>`
//!
//! The crate rules below are the source of truth for architectural layering.
//! They mirror `PROJECT.md`; a new crate must be registered in both places in
//! the same change.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, Stdio};

/// Dependencies that the domain side of the workspace must never pull in.
const UI_GPU_DEPS: &[&str] = &["egui", "eframe", "egui-wgpu", "wgpu", "winit"];

/// Crates that may only be reached through a dev-dependency.
const DEV_ONLY: &[&str] = &["furnicraft-testkit"];

/// The third-party geometry kernel is an implementation detail of its adapter.
const TRUCK_PREFIX: &str = "truck-";
const TRUCK_ADAPTER: &str = "furnicraft-kernel-truck";

/// Crates that only the composition root may wire in as normal dependencies.
const COMPOSITION_ONLY: &[&str] = &[TRUCK_ADAPTER];

/// Crates that compile or vendor C/C++ sources. OS bindings that only load a
/// platform library through Rust FFI are not listed here: the rule is about
/// native code shipped and built with FurniCraft, not about the operating system.
const C_VENDORING_CRATES: &[&str] =
    &["bindgen", "bzip2-sys", "cmake", "libz-sys", "lzma-sys", "openssl-sys", "vtkio", "xz2", "zstd-sys"];

/// The gate, in order. Each step must pass before the next one starts.
/// `layers` is not a cargo command and runs in-process after these.
const CI_STEPS: &[(&str, &[&str])] = &[
    ("fmt", &["fmt", "--all", "--check"]),
    ("clippy", &["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"]),
    ("test", &["test", "--workspace"]),
];

/// One workspace crate: its layer and the dependencies it must never have.
///
/// A crate may depend only on crates in lower layers; `forbidden` covers edges
/// that the layer rule cannot express: external crates, and lower-layer crates
/// that are still the wrong dependency.
struct CrateRule {
    name: &'static str,
    layer: u8,
    forbidden: &'static [&'static str],
}

/// The crate map. Keep in sync with `PROJECT.md` › Crate map.
const CRATES: &[CrateRule] = &[
    CrateRule { name: "furnicraft-units", layer: 0, forbidden: UI_GPU_DEPS },
    CrateRule { name: "furnicraft-geom", layer: 0, forbidden: UI_GPU_DEPS },
    // The geometry port. It must not know the third-party engine: the global
    // `TRUCK_PREFIX` rule below keeps truck out of every crate but the adapter.
    CrateRule { name: "furnicraft-kernel", layer: 1, forbidden: UI_GPU_DEPS },
    CrateRule { name: "furnicraft-model", layer: 1, forbidden: UI_GPU_DEPS },
    // The only crate allowed to depend on truck.
    CrateRule { name: TRUCK_ADAPTER, layer: 2, forbidden: UI_GPU_DEPS },
    CrateRule { name: "furnicraft-param", layer: 2, forbidden: UI_GPU_DEPS },
    CrateRule { name: "furnicraft-project", layer: 2, forbidden: UI_GPU_DEPS },
    CrateRule { name: "furnicraft-mesh", layer: 3, forbidden: UI_GPU_DEPS },
    CrateRule { name: "furnicraft-scene", layer: 4, forbidden: UI_GPU_DEPS },
    // The renderer draws the scene; it must stay egui-free (the app owns the
    // paint callback) and must not reach into the document.
    CrateRule {
        name: "furnicraft-render",
        layer: 5,
        forbidden: &["egui", "eframe", "egui-wgpu", "winit", "furnicraft-model"],
    },
    CrateRule { name: "furnicraft-engine", layer: 6, forbidden: UI_GPU_DEPS },
    // The UI owns egui but never the GPU: the shell wires the renderer in.
    CrateRule { name: "furnicraft-ui", layer: 7, forbidden: &["wgpu", "egui-wgpu", "winit"] },
    // The native shell is the composition root; layers alone constrain it.
    CrateRule { name: "furnicraft", layer: 8, forbidden: &[] },
    // Development-only helpers may use anything; product crates may only reach
    // them through a dev-dependency (see `DEV_ONLY`).
    CrateRule { name: "furnicraft-testkit", layer: 9, forbidden: &[] },
    // The plan gate is a generic tool: at L0 it can depend on no product crate
    // at all, which is exactly the rule the extracted crate must keep.
    CrateRule { name: "state-contract", layer: 0, forbidden: UI_GPU_DEPS },
];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("layers") => report(check_layers()),
        Some("plan") => report_plan(check_plan()),
        Some("status") => print_status(),
        Some("dashboard") => print_dashboard(),
        Some("ci") => run_ci(),
        Some("help") | Some("--help") | None => {
            print_usage();
            0
        }
        Some(other) => {
            eprintln!("xtask: unknown task `{other}`");
            print_usage();
            2
        }
    };
    std::process::exit(code);
}

fn print_usage() {
    println!("usage: cargo xtask <layers|plan|status|dashboard|ci>");
    println!("  layers    check crate rules: layering, forbidden/dev-only dependencies, and no C/C++ sources");
    println!("  plan      check that plan/STATUS.md, plan/BACKLOG.md and plan/SESSION.md agree");
    println!("  status    print the current task and what follows, for a session start");
    println!("  dashboard print a styled overview of the plan; each section names its source file");
    println!("  ci        run the whole gate: fmt, clippy, tests, layers, plan");
}

/// Runs the gate in order and returns the process exit code.
fn run_ci() -> i32 {
    let workspace = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
    for (name, args) in CI_STEPS {
        println!("ci: {name}");
        let status = Command::new(env!("CARGO"))
            .args(*args)
            .current_dir(workspace)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status();
        let status = match status {
            Ok(status) => status,
            Err(error) => {
                eprintln!("ci: FAILED at {name}: cannot run cargo: {error}");
                return 1;
            }
        };
        if !status.success() {
            eprintln!("ci: FAILED at {name}");
            return status.code().unwrap_or(1);
        }
    }

    if report(check_layers()) != 0 {
        return 1;
    }
    if report_plan(check_plan()) != 0 {
        return 1;
    }
    println!("ci: ok");
    0
}

/// The workspace root, one level above `xtask`.
fn workspace_root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
}

/// Checks the plan files in the workspace root.
fn check_plan() -> Result<state_contract::Summary, Vec<String>> {
    state_contract::check(workspace_root())
}

/// Prints what the plan files say, without failing on a warning.
fn print_status() -> i32 {
    match state_contract::status(workspace_root()) {
        Ok(text) => {
            print!("{text}");
            0
        }
        Err(violations) => report_plan(Err(violations)),
    }
}

/// Prints the styled overview; every section names the file it was read from. A thin wrapper:
/// all the layout lives in `state_contract::dashboard`.
fn print_dashboard() -> i32 {
    let opts = state_contract::DashboardOptions::default();
    match state_contract::dashboard(workspace_root(), &opts) {
        Ok(text) => {
            println!("{text}");
            0
        }
        Err(violations) => {
            for violation in &violations {
                eprintln!("plan: {violation}");
            }
            violations.len() as i32
        }
    }
}

fn report_plan(result: Result<state_contract::Summary, Vec<String>>) -> i32 {
    match result {
        Ok(summary) => {
            println!("plan: {} tasks, state {}", summary.tasks.len(), summary.state.status);
            println!("plan: ok");
            0
        }
        Err(violations) => {
            for violation in &violations {
                eprintln!("error: {violation}");
            }
            eprintln!("plan: FAILED ({} violation(s))", violations.len());
            1
        }
    }
}

fn report(result: Result<usize, Vec<String>>) -> i32 {
    match result {
        Ok(checked) => {
            println!("layers: {checked} crates checked");
            println!("layers: ok");
            0
        }
        Err(violations) => {
            for violation in &violations {
                eprintln!("error: {violation}");
            }
            eprintln!("layers: FAILED ({} violation(s))", violations.len());
            1
        }
    }
}

fn check_layers() -> Result<usize, Vec<String>> {
    let metadata = cargo_metadata().map_err(|error| vec![error])?;
    let packages = array(&metadata, "packages").map_err(|error| vec![error])?;
    let (checked, mut violations) = collect_violations(CRATES, packages).map_err(|error| vec![error])?;
    violations.extend(c_vendoring_violations(packages));

    if violations.is_empty() { Ok(checked) } else { Err(violations) }
}

/// Rejects dependencies that build or ship native C/C++ code.
fn c_vendoring_violations(packages: &[serde_json::Value]) -> Vec<String> {
    packages
        .iter()
        .filter_map(|package| {
            let name = package.get("name")?.as_str()?;
            C_VENDORING_CRATES
                .contains(&name)
                .then(|| format!("`{name}` compiles or vendors C/C++ code; FurniCraft dependencies must be pure Rust"))
        })
        .collect()
}

/// Checks every workspace package against the crate rules.
///
/// Returns the number of crates that were checked and every violation found.
fn collect_violations(rules: &[CrateRule], packages: &[serde_json::Value]) -> Result<(usize, Vec<String>), String> {
    let layers: BTreeMap<&str, u8> = rules.iter().map(|rule| (rule.name, rule.layer)).collect();
    let mut checked = 0usize;
    let mut violations = Vec::new();

    for package in packages {
        let name = string(package, "name")?;

        let Some(rule) = rules.iter().find(|rule| rule.name == name) else {
            // The task runner itself is not part of the product layering.
            if name != "xtask" {
                violations.push(format!(
                    "`{name}` is not registered in the crate rules (`xtask/src/main.rs`); add it there and to `PROJECT.md`"
                ));
            }
            continue;
        };
        checked += 1;

        for dependency in array(package, "dependencies")? {
            let dependency_name = string(dependency, "name")?;
            let kind = dependency.get("kind").and_then(serde_json::Value::as_str);

            if DEV_ONLY.contains(&dependency_name) {
                if kind != Some("dev") {
                    violations.push(format!(
                        "`{dependency_name}` is development-only and must not be a {} dependency of `{name}`",
                        kind.unwrap_or("normal")
                    ));
                }
                continue;
            }

            // The third-party geometry engine is a detail of its adapter. No other
            // crate — not even in tests — may reach it directly.
            if dependency_name.starts_with(TRUCK_PREFIX) && name != TRUCK_ADAPTER {
                violations.push(format!(
                    "`{name}` must not depend on `{dependency_name}`: only `{TRUCK_ADAPTER}` may use the geometry engine"
                ));
            }

            // The adapter is composed once, in the native shell; other crates may use it only
            // from tests as a real-kernel fixture.
            if COMPOSITION_ONLY.contains(&dependency_name) && name != "furnicraft" && kind != Some("dev") {
                violations.push(format!(
                    "`{name}` must not depend on `{dependency_name}`: only the app composes the kernel adapter"
                ));
            }

            // Dev- and build-dependencies may point anywhere else: they do not ship.
            if matches!(kind, Some("dev" | "build")) {
                continue;
            }

            if let Some(&dependency_layer) = layers.get(dependency_name)
                && dependency_layer >= rule.layer
            {
                violations.push(format!(
                    "`{name}` (L{}) depends on `{dependency_name}` (L{dependency_layer}): \
                     a crate may only depend on lower layers",
                    rule.layer
                ));
            }

            if rule.forbidden.contains(&dependency_name) {
                violations.push(format!(
                    "`{name}` must not depend on `{dependency_name}` \
                     (see `PROJECT.md` › Crate map and `AGENTS.md` §3)"
                ));
            }
        }
    }

    Ok((checked, violations))
}

fn cargo_metadata() -> Result<serde_json::Value, String> {
    let manifest_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../Cargo.toml");
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version=1", "--no-deps", "--manifest-path", manifest_path])
        .output()
        .map_err(|error| format!("cannot run `cargo metadata`: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("`cargo metadata` failed:\n{stderr}"));
    }

    serde_json::from_slice(&output.stdout).map_err(|error| format!("cannot parse `cargo metadata` output: {error}"))
}

fn array<'a>(value: &'a serde_json::Value, key: &str) -> Result<&'a Vec<serde_json::Value>, String> {
    value.get(key).and_then(serde_json::Value::as_array).ok_or_else(|| format!("`cargo metadata` has no `{key}` array"))
}

fn string<'a>(value: &'a serde_json::Value, key: &str) -> Result<&'a str, String> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| format!("`cargo metadata` field `{key}` is not a string"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn package(name: &str, dependencies: &[(&str, Option<&str>)]) -> serde_json::Value {
        let dependencies: Vec<serde_json::Value> =
            dependencies.iter().map(|(name, kind)| json!({ "name": name, "kind": kind })).collect();
        json!({ "name": name, "dependencies": dependencies })
    }

    fn collect(packages: &[serde_json::Value]) -> Vec<String> {
        let (_, violations) = collect_violations(CRATES, packages).expect("synthetic metadata is valid");
        violations
    }

    #[test]
    fn the_gate_keeps_every_step() {
        let names: Vec<&str> = CI_STEPS.iter().map(|(name, _)| *name).collect();
        assert_eq!(names, ["fmt", "clippy", "test"]);
        assert!(CI_STEPS[1].1.ends_with(&["-D", "warnings"]));
    }

    #[test]
    fn fmt_is_checked_not_rewritten() {
        assert!(CI_STEPS[0].1.contains(&"--check"));
    }

    #[test]
    fn renderer_must_not_depend_on_egui() {
        let violations = collect(&[package("furnicraft-render", &[("egui", None), ("wgpu", None)])]);
        assert!(
            violations.iter().any(|violation| violation.contains("`furnicraft-render` must not depend on `egui`")),
            "{violations:?}"
        );
    }

    #[test]
    fn ui_must_not_depend_on_wgpu() {
        let violations = collect(&[package("furnicraft-ui", &[("wgpu", None)])]);
        assert!(
            violations.iter().any(|violation| violation.contains("`furnicraft-ui` must not depend on `wgpu`")),
            "{violations:?}"
        );
    }

    #[test]
    fn domain_crates_must_not_depend_on_ui_or_gpu() {
        let violations = collect(&[package("furnicraft-model", &[("wgpu", None)])]);
        assert!(
            violations.iter().any(|violation| violation.contains("`furnicraft-model` must not depend on `wgpu`")),
            "{violations:?}"
        );
    }

    #[test]
    fn crates_may_not_depend_on_their_own_layer_or_above() {
        let violations = collect(&[package("furnicraft-engine", &[("furnicraft-ui", None)])]);
        assert!(
            violations.iter().any(|violation| violation.contains("a crate may only depend on lower layers")),
            "{violations:?}"
        );
    }

    #[test]
    fn testkit_is_dev_only() {
        let normal = collect(&[package("furnicraft-model", &[("furnicraft-testkit", None)])]);
        assert!(normal.iter().any(|violation| violation.contains("development-only")), "{normal:?}");

        let dev = collect(&[package("furnicraft-model", &[("furnicraft-testkit", Some("dev"))])]);
        assert!(dev.is_empty(), "{dev:?}");
    }

    #[test]
    fn only_the_adapter_may_use_truck() {
        let leaked = collect(&[package("furnicraft-model", &[("truck-modeling", None)])]);
        assert!(
            leaked.iter().any(|violation| violation.contains("only `furnicraft-kernel-truck` may use")),
            "{leaked:?}"
        );

        let adapter = collect(&[package("furnicraft-kernel-truck", &[("truck-modeling", None)])]);
        assert!(adapter.is_empty(), "{adapter:?}");
    }

    #[test]
    fn only_the_app_composes_the_adapter() {
        let leaked = collect(&[package("furnicraft-engine", &[("furnicraft-kernel-truck", None)])]);
        assert!(leaked.iter().any(|violation| violation.contains("only the app composes")), "{leaked:?}");

        let app = collect(&[package("furnicraft", &[("furnicraft-kernel-truck", None)])]);
        assert!(app.is_empty(), "{app:?}");

        let dev = collect(&[package("furnicraft-mesh", &[("furnicraft-kernel-truck", Some("dev"))])]);
        assert!(dev.is_empty(), "{dev:?}");
    }

    #[test]
    fn c_vendoring_dependency_is_rejected() {
        let packages = [serde_json::json!({ "name": "lzma-sys" })];
        let violations = c_vendoring_violations(&packages);
        assert!(violations.iter().any(|violation| violation.contains("must be pure Rust")), "{violations:?}");

        let packages = [serde_json::json!({ "name": "serde" })];
        assert!(c_vendoring_violations(&packages).is_empty());
    }

    #[test]
    fn unregistered_crate_fails() {
        let violations = collect(&[package("furnicraft-unknown", &[])]);
        assert!(
            violations.iter().any(|violation| violation.contains("is not registered in the crate rules")),
            "{violations:?}"
        );
    }

    #[test]
    fn the_plan_gate_crate_must_not_depend_on_the_product() {
        let violations = collect(&[package("state-contract", &[("furnicraft-model", None)])]);
        assert!(
            violations.iter().any(|violation| violation.contains("a crate may only depend on lower layers")),
            "{violations:?}"
        );

        let clean = collect(&[package("state-contract", &[])]);
        assert!(clean.is_empty(), "{clean:?}");
    }

    #[test]
    fn clean_workspace_passes() {
        let packages = [
            package("furnicraft-units", &[("serde", None)]),
            package("furnicraft-model", &[("furnicraft-units", None), ("serde", None)]),
            package("furnicraft-render", &[("wgpu", None), ("furnicraft-scene", None)]),
            package("furnicraft-testkit", &[("furnicraft-model", None), ("egui", None)]),
            package("xtask", &[("serde_json", None)]),
        ];
        let (checked, violations) = collect_violations(CRATES, &packages).expect("synthetic metadata is valid");
        assert_eq!(checked, 4, "xtask is not part of the crate rules");
        assert!(violations.is_empty(), "{violations:?}");
    }
}
