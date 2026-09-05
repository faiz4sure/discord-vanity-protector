# Building and Compilation Guide

This document outlines prerequisites, build configurations, dual artifact generation, linting standards, and cross-compilation workflows for the Discord Vanity Protector (DVP) engine.

---

## 1. Prerequisites

### System Requirements
- **Rust Toolchain**: Rust 1.80.0+ (stable channel) with `cargo`, `clippy`, and `rustfmt`
- **C/C++ Compiler**: GCC 10+ or Clang 14+
- **Build Utilities**: `make`, `cmake`, `pkg-config`
- **OpenSSL / BoringSSL Build Dependencies**: `libssl-dev` (Linux Debian/Ubuntu) or `openssl-devel` (Fedora/RHEL)

### Platform-Specific Setup

#### Ubuntu / Debian
```bash
sudo apt-get update
sudo apt-get install -y build-essential pkg-config libssl-dev cmake clang
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
rustup component add clippy rustfmt
```

#### Arch Linux
```bash
sudo pacman -Syu --needed base-devel openssl cmake clang rustup
rustup default stable
rustup component add clippy rustfmt
```

#### macOS (Apple Silicon / Intel)
```bash
xcode-select --install
brew install cmake openssl
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
rustup component add clippy rustfmt
```

#### Windows (MSVC Toolchain)
1. Install [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) with the "Desktop development with C++" workload.
2. Install Rust via [rustup.rs](https://rustup.rs/).
3. Ensure `clippy` and `rustfmt` components are installed:
   ```powershell
   rustup component add clippy rustfmt
   ```

---

## 2. Release Compilation

To compile DVP with maximum optimization:

```bash
cargo build --release
```

### Compiler Profile Optimization (`Cargo.toml`)

The release profile in `Cargo.toml` is tuned for low-latency network I/O and minimal binary overhead:

```toml
[profile.release]
opt-level = 3        # Maximum compiler optimizations (-O3)
lto = "fat"          # Full Link-Time Optimization across all crates
codegen-units = 1    # Single codegen unit enabling aggressive inter-procedural inlining
panic = "abort"      # Disables unwinding tables, shrinks binary, optimizes hot paths
strip = true         # Strips debug symbols and symbol tables from release binaries
```

### High-Performance Allocator (`mimalloc`)

DVP utilizes `mimalloc` as the global memory allocator (`src/main.rs`), eliminating thread lock contention during high-frequency JSON parsing and asynchronous socket event processing:

```rust
use mimalloc::MiMalloc;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;
```

---

## 3. Dual Artifact Generation

DVP compiles simultaneously as a standalone executable and a C-compatible dynamic shared library through the `[lib]` crate configuration:

```toml
[lib]
name = "dvp"
crate-type = ["rlib", "cdylib"]
```

When running `cargo build --release`, Cargo produces two primary output artifacts in `target/release/`:

| Platform | Standalone Executable | Dynamic Shared Library (C FFI) |
| :--- | :--- | :--- |
| **Linux** (`x86_64` / `aarch64`) | `target/release/dvp` | `target/release/libdvp.so` |
| **Windows** (`x86_64`) | `target/release/dvp.exe` | `target/release/dvp.dll` |
| **macOS** (`ARM64` / `x86_64`) | `target/release/dvp` | `target/release/libdvp.dylib` |

- **Standalone Executable (`dvp`)**: Directly runs the Tokio runtime and reads `config.toml`.
- **Shared Library (`libdvp`)**: Exports the C ABI entry point `dvp_start(const char *config_path)` for embedding into Python, Bun, Node.js, Go, or C/C++ runtimes.

---

## 4. Code Quality & Formatting Standards

The CI pipeline enforces strict zero-warning compilation and code style rules.

### Formatting Verification
Run `rustfmt` using the repository's formatting configuration (`rustfmt.toml`):

```bash
cargo fmt --check
```

To auto-format all code files:
```bash
cargo fmt
```

### Clippy Linting
Run Clippy across all targets with denial on all warnings:

```bash
cargo clippy --all-targets -- -D warnings
```

Configuration parameters enforced via `.clippy.toml`:
- `cognitive-complexity-threshold = 30`
- `too-many-lines-threshold = 500`
- `too-many-arguments-threshold = 8`

---

## 5. Cross-Compilation Workflows

### Target Triples

| Target Triple | Target Platform | Architecture |
| :--- | :--- | :--- |
| `x86_64-unknown-linux-gnu` | Linux Standard (Ubuntu, Debian, Arch) | 64-bit x86 |
| `aarch64-unknown-linux-gnu` | Linux ARM / Android Termux / Raspberry Pi | 64-bit ARM |
| `x86_64-pc-windows-msvc` | Windows Desktop / Server (MSVC ABI) | 64-bit x86 |
| `x86_64-pc-windows-gnu` | Windows Desktop (MinGW ABI) | 64-bit x86 |
| `aarch64-apple-darwin` | macOS Apple Silicon (M1/M2/M3/M4) | 64-bit ARM |
| `x86_64-apple-darwin` | macOS Intel | 64-bit x86 |

---

### Cross-Compiling for Linux ARM64 (`aarch64-unknown-linux-gnu`)

From an Ubuntu/Debian x86_64 host:

```bash
# 1. Install target toolchain and cross-linker
sudo apt-get update
sudo apt-get install -y gcc-aarch64-linux-gnu g++-aarch64-linux-gnu
rustup target add aarch64-unknown-linux-gnu

# 2. Compile with linker override
CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
cargo build --release --target aarch64-unknown-linux-gnu
```

The resulting binaries will be available at:
- `target/aarch64-unknown-linux-gnu/release/dvp`
- `target/aarch64-unknown-linux-gnu/release/libdvp.so`

---

### Cross-Compiling for Windows (`x86_64-pc-windows-gnu`) via MinGW

From a Linux host:

```bash
# 1. Install MinGW toolchain
sudo apt-get install -y mingw-w64
rustup target add x86_64-pc-windows-gnu

# 2. Compile target
cargo build --release --target x86_64-pc-windows-gnu
```

---

### Cross-Compiling with `cargo-zigbuild` (Multi-Platform)

`cargo-zigbuild` allows cross-compilation across glibc and musl versions without installing separate GNU toolchains:

```bash
# 1. Install zig and cargo-zigbuild
pip install cargo-zigbuild
rustup target add aarch64-unknown-linux-gnu x86_64-unknown-linux-gnu x86_64-pc-windows-gnu

# 2. Build for specific glibc baseline (e.g., glibc 2.17 for broad Linux compatibility)
cargo zigbuild --release --target aarch64-unknown-linux-gnu.2.17
cargo zigbuild --release --target x86_64-unknown-linux-gnu.2.17
cargo zigbuild --release --target x86_64-pc-windows-gnu
```

---

### Cross-Compiling with `cross` (Docker-based)

For automated containerized compilation:

```bash
# 1. Install cross CLI
cargo install cross --git https://github.com/cross-rs/cross

# 2. Build for desired targets
cross build --release --target aarch64-unknown-linux-gnu
cross build --release --target x86_64-pc-windows-gnu
```

