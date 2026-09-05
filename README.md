# Discord Vanity Protector (DVP)

A high-performance Discord vanity URL protector written in Rust. DVP continuously monitors your Discord server in real-time, instantly detects unauthorized vanity URL changes, automatically restores your vanity URL using account password verification, and punishes attackers with zero delay.

DVP appears as an official Discord Windows Desktop client with full TLS emulation, preventing bot detection.

---

## Features

- **Instant Vanity Reversion**: Restores your vanity URL in milliseconds when an unauthorized change occurs.
- **Automated 2FA / Password Verification**: Uses your account password to complete verification on-demand and caches the security token in memory for fast recovery.
- **Concurrent Attacker Punishment**: Bans or kicks the attacker at the same instant the vanity is restored.
- **Zero-Setup Run (No Rust Required)**: You can run DVP directly with Python, Node.js, or Bun — the runner automatically downloads the pre-built engine for your operating system.
- **Desktop Client Impersonation**: Uses official Discord Windows Desktop headers, build numbers, and BoringSSL TLS fingerprints.
- **Token Health Monitor**: Periodically verifies your account token in the background and sends an emergency webhook alert if it expires.
- **24/7 Resilience**: Automatic reconnection and session resume during network drops or Discord gateway restarts.

---

## Quick Start

### 1. Clone the Repository
```bash
git clone https://github.com/faiz4sure/discord-vanity-protector.git
cd discord-vanity-protector
```

### 2. Configure Settings
Open `config.toml` in any text editor and fill in your details (your Discord account token, server ID, vanity code, account password, etc.). 

*Every setting has clear, detailed comments written inside `config.toml` explaining what it does.*

### 3. Run

You can run DVP using any of the following methods. On first launch, Python/Node/Bun will automatically download the pre-built native engine for your platform:

#### Python
```bash
python3 main.py
```

#### Node.js
```bash
npm install
node index.js
```

#### Bun
```bash
bun run bun.ts
```

#### Standalone Binary (Rust)
```bash
cargo run --release
# or run the downloaded binary directly:
./dvp
```

---

## Protection Modes

You can select your preferred protection mode in `config.toml`:

- **`audit` (Recommended)**: Detects the attacker immediately through Discord audit log gateway events and executes both vanity recovery and attacker punishment simultaneously.
- **`normal`**: Reverts the vanity URL upon server update events and looks up the attacker via audit log history.
- **`fast`**: Reverts the vanity URL in sub-milliseconds without waiting to identify or punish the attacker (maximum speed recovery).

---

## Documentation

For deep-dive guides and architecture specifications:

- [Architecture & Pipeline](docs/ARCHITECTURE.md): Full end-to-end engine flow, Mermaid diagrams, and module breakdown.
- [Building from Source](docs/BUILD.md): Compiler flags, cross-compilation, and release optimizations.
- [Android (Termux) Guide](docs/ANDROID.md): Setup and running DVP on Android devices.
- [Extracting Discord Token](docs/GET_TOKEN.md): Safe step-by-step tutorial on retrieving your account token.

---

## Support & Community

If you need help setting up, encounter unexpected errors, or have questions:

- **Discord Support Server**: [https://discord.gg/nreK8UQwHW](https://discord.gg/nreK8UQwHW)
- **GitHub Issues**: Open an issue on this repository.

---

## License

This project is licensed under the [GNU Affero General Public License v3.0](LICENSE).
