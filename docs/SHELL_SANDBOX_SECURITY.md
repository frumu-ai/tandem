# Shell Sandbox Security

Tandem treats the `bash` tool as a high-risk external-effect capability. The
runtime must provide explicit workspace context before shell execution, and
platforms without an available sandbox fail closed unless
`TANDEM_UNSAFE_UNSANDBOXED_SHELL` is intentionally enabled.

## Linux

Linux shell execution uses bubblewrap when `bwrap` is available. The sandbox
binds the workspace root read-write, mounts `/tmp` as a private tmpfs, binds
standard system directories read-only, sets `HOME` to the workspace root, and
runs the command through `/bin/sh -lc`.

The default network policy is no host network access. The bubblewrap argv uses
`--unshare-all` and does not pass `--share-net`, so any future change that
shares host networking should appear as an intentional test diff and release
note.

## macOS and Other Unix Platforms

Unix platforms without the Linux bubblewrap sandbox refuse shell execution by
default. Operators can set `TANDEM_UNSAFE_UNSANDBOXED_SHELL=1` for local
development, but that mode is marked as an unsafe sandbox opt-out in tool
metadata.

## Windows

Windows shell execution goes through the PowerShell guardrail. Known safe POSIX
inspection commands such as `ls` and `find` are translated to PowerShell, while
Unix-only commands such as `sed`, `bash`, and direct POSIX path patterns are
blocked with a clear guardrail reason.
