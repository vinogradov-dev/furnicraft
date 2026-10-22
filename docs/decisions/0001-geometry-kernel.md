# 0001. The geometry kernel sits behind a port

- Status: Accepted
- Date: 2026-10-22

## Context

FurniCraft needs extrusion, booleans, tessellation and measures. Every candidate engine is a large
dependency with its own types, error model and failure modes, and dependencies may not compile C or
C++ (`AGENTS.md` §2, §4.8). The document, parametric evaluation and the command layer must not depend
on whichever engine is installed today.

## Decision

`furnicraft-kernel` defines the port in our own types: profiles, opaque bodies, booleans, tessellation
and measures, errors as `KernelError`. `furnicraft-kernel-truck` is the only crate that may depend on
`truck`; the application composes it and injects it into the session.

## Consequences

- `model`, `param` and `engine` never see a third-party geometry type, so the port can be
  re-implemented without touching them.
- Every call crosses a millimetre boundary: `param` converts `Length` to `f64` millimetres on the way
  in, and results come back as our own `MeshData`.
- The adapter guards engine calls, so a panic inside the engine becomes `KernelError::Internal`
  instead of a crash.
- An operation the engine cannot do returns `KernelError::Unsupported`, never a silent approximation.
- `cargo xtask layers` enforces the boundary: no crate but the adapter may depend on `truck-*`.
