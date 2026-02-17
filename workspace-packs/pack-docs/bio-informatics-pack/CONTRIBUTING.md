# Contributing (Bio-Informatics Pack)

## Python venv standard

Do not use global `pip install ...` instructions in this pack's docs.

Always target the workspace venv:

### Windows

```bash
cd "<your-pack-folder>"
.tandem\.venv\Scripts\python.exe -m pip install -r requirements.txt
```

### macOS / Linux

```bash
cd "<your-pack-folder>"
.tandem/.venv/bin/python3 -m pip install -r requirements.txt
```

## Pack update rules

1. Keep examples synthetic and safe to publish.
2. Keep references focused and actionable.
3. Prefer deterministic script behavior and clear logging.
4. Document new dependencies and tool assumptions.
