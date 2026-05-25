# Contributing (Data Visualization Pack)

## Python venv standard

This pack includes Python templates. Do not recommend global `pip install ...` in docs.

Always assume dependencies must be installed into the workspace venv at `.opencode/.venv`.

### Windows

```bash
cd "<your-pack-folder>"
.opencode\.venv\Scripts\python.exe -m pip install -r requirements.txt
```

### macOS / Linux

```bash
cd "<your-pack-folder>"
.opencode/.venv/bin/python3 -m pip install -r requirements.txt
```

## requirements.txt

If you add new Python dependencies:

1. Add them to `requirements.txt`.
2. Keep dependencies minimal; avoid pinning unless necessary.
3. If pinning is required, explain why in the PR description.
