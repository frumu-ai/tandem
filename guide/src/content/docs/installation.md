---
title: Installation & Build
---

## Downloading Binaries

Pre-compiled binaries for **Windows**, **macOS**, and **Linux** are available on our [GitHub Releases](https://github.com/tandem-engine/tandem/releases) page.

1. Download the archive for your operating system.
2. Extract the contents.
   - You will find `tandem-engine` (the core server) and `tandem-tui` (the terminal interface).
3. Add the extraction directory to your system's `PATH` for easy access.

## Building from Source

To build Tandem from source, you need **Rust** installed (stable channel).

1. **Clone the repository**:

   ```bash
   git clone https://github.com/tandem-engine/tandem.git
   cd tandem
   ```

2. **Build with Cargo**:

   ```bash
   cargo build --release
   ```

   - The binaries will be located in `target/release/`.

3. **Run**:
   ```bash
   ./target/release/tandem-engine
   ```
