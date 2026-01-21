# Tiny Ollama Models for Local Testing

This doc lists small Ollama models that are good for quick local testing on laptops, plus setup steps for Tandem.

## Quick Recommendations (Small and Fast)

These are the smallest common models that still work for basic chat and UI testing:

- `qwen2.5:0.5b` (very small, fast, lowest quality)
- `tinyllama:1.1b` (tiny, fast, basic responses)
- `llama3.2:1b` (small, better than tinyllama, still light)
- `gemma:2b` (small, decent quality, slower than 1B models)
- `phi3:mini` (small but sharper on short tasks)

## Will They Run on a Laptop?

Yes, these should run on most modern laptops, but performance depends on RAM and CPU:

- 8 GB RAM: 0.5B to 1.1B models are usually OK
- 16 GB RAM: 1B to 2B models are generally OK
- Expect slower responses on older CPUs (but fine for QA/testing)

If you see the model loading slowly or the app feels laggy, start with `qwen2.5:0.5b` or `tinyllama:1.1b`.

## Setup Steps (Ollama + Tandem)

1) Install Ollama (Linux):

```bash
curl -fsSL https://ollama.com/install.sh | sh
```

2) Start Ollama:

```bash
ollama serve
```

3) Pull a tiny model (pick one):

```bash
ollama pull qwen2.5:0.5b
ollama pull tinyllama:1.1b
ollama pull llama3.2:1b
```

4) In Tandem, open Settings and enable the Ollama provider:

- **Endpoint:** `http://localhost:11434`
- **Model:** one of the model names above (exact string)

5) Test in chat:

- "Write a two sentence summary of this app."
- "List three features in this project."
- "Explain the last error message I saw."

## Notes / Gotchas

- If the model name is wrong, Ollama returns an error. Use the exact tag from `ollama list`.
- You can verify the model is installed with:
  ```bash
  ollama list
  ```
- Small models are fine for UI testing but not ideal for real reasoning.
- If Ollama is not running, Tandem will show provider errors.

