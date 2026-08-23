# Rust toolchain on Windows: MSVC or GNU

Type: task
Status: closed
Blocked by: none

## Question

There is no Rust toolchain in this environment yet, and both spikes need one.
This blocks `03 Spike: Win32 Job Object kill-on-close with a real harness child` and `04 Spike: workspace create and destroy under a running dev server`.

The choice is between two Tier 1 Rust targets, plus a zig variant:

- `x86_64-pc-windows-msvc`. The Rust default on Windows. Requires Visual Studio Build Tools with the C++ workload and the Windows SDK, which is a large install and normally needs administrator rights.
- `x86_64-pc-windows-gnu`. MinGW-w64. Available as `w64devkit`, a single portable zip with no installer and no administrator rights.
- `cargo-zigbuild` with `zig cc` as the linker. Mainly a cross-compilation tool.

The tension is real and worth recording rather than deciding by default.
The operator's standing preference is portable, user-space, offline-friendly tooling with no privileged installers, which points at GNU.
Farseer's entire value proposition is Windows-native correctness, which points at the toolchain the rest of the ecosystem tests against.

Points to settle:

- Does `windows-rs` behave identically on both targets? It generates its own bindings and ships its own import libraries, so it needs no C headers, which weakens the usual argument that Win32 work requires the Windows SDK.
- Debugging. MSVC produces PDB files and works with the Windows debuggers and Visual Studio. GNU produces DWARF, which has patchier tooling on Windows.
- Distribution. MSVC binaries link the VC runtime dynamically unless `+crt-static` is set. GNU binaries may need `libgcc` and `libwinpthread` DLLs unless statically linked. Farseer ships a single binary, so this matters.
- Ecosystem risk. Crates are overwhelmingly tested on MSVC first. Being on the less-travelled target means hitting bugs nobody else hits, which is precisely the failure mode farseer exists to eliminate.
- Whether Visual Studio Build Tools can be installed without administrator rights in this environment, which decides whether the tension is real or theoretical.

Resolved when a toolchain is installed, `cargo --version` and `rustc --version` report cleanly, and a hello-world binary using one `windows-rs` call compiles and runs.
Record the target triple chosen and the reason, since every later build assumes it.

## Resolution

Resolved 2026-08-22.

### Target triple

**`x86_64-pc-windows-msvc`**, via `rustup` default stable.

```
rustc 1.98.0 (88d9e12ae 2026-08-18)
cargo 1.98.0 (797e8a9bc 2026-08-05)
Default host: x86_64-pc-windows-msvc
```

The tension recorded above turned out to be cheaper than feared.
Visual Studio Build Tools 2026 (`Microsoft.VisualStudio.BuildTools`, winget, no year suffix in the package id) installed with two components only:

- `Microsoft.VisualStudio.Component.VC.Tools.x86.x64` - the linker.
- `Microsoft.VisualStudio.Component.Windows11SDK.26100` - the import libraries.

No `--includeRecommended`, no `Microsoft.VisualStudio.Workload.VCTools` workload.
`rustup` itself needs no administrator rights.

### Why MSVC over GNU

Ecosystem risk decided it.
Crates are tested on MSVC first, and farseer's entire value proposition is Windows-native correctness.
Being on the less-travelled target would mean hitting bugs nobody else hits, which is exactly the failure mode farseer exists to eliminate.
PDB debugging and single-binary distribution without `libgcc` and `libwinpthread` were the tie-breakers.

The portability preference is not abandoned, it is relocated: it applies to farseer's own runtime dependencies, not to the compiler that builds it once.

### Verification

`windows-rs` v0.62.2 compiles and runs against Win32 Job Objects:

```rust
use windows::Win32::System::JobObjects::CreateJobObjectW;
use windows::Win32::Foundation::CloseHandle;

fn main() -> windows::core::Result<()> {
    let job = unsafe { CreateJobObjectW(None, None)? };
    println!("job handle ok: {:?}", job);
    unsafe { CloseHandle(job)? };
    Ok(())
}
```

Output: `job handle ok: HANDLE(0x13c)` then `closed`.

### One fact carried to `03`

`windows-rs` feature gates are finer than the module path suggests.
`CreateJobObjectW` lives in `Win32::System::JobObjects` but is gated behind **`Win32_Security`**, because its first parameter is a `SECURITY_ATTRIBUTES` pointer.
Enabling `Win32_System_JobObjects` alone produces a misleading "no `CreateJobObjectW` in this module" error.
Required feature set for the spike: `Win32_Foundation`, `Win32_System_JobObjects`, `Win32_Security`, and `Win32_System_Threading` for `CreateProcessW`.
