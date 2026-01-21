# Using Ollama with Tandem

Tandem seamlessly integrates with [Ollama](https://ollama.com/) to let you run powerful LLMs locally on your machine, free of charge and with complete privacy.

## 1. Install Ollama

### macOS
1.  Download Ollama from [ollama.com/download/mac](https://ollama.com/download/mac).
2.  Install the application.
3.  Open your terminal and verify installation:
    ```bash
    ollama --version
    ```

### Linux
1.  Install via the official script:
    ```bash
    curl -fsSL https://ollama.com/install.sh | sh
    ```
2.  Verify running status:
    ```bash
    systemctl status ollama
    ```
    *(If you want to run without systemd, follow the manual instructions on their GitHub).*

### Windows
1.  Download the installer from [ollama.com/download/windows](https://ollama.com/download/windows).
2.  Run the `.exe` file.

---

## 2. Pull a Model

Tandem automatically detects any model you have installed. We recommend **GLM-4** or **Llama 3** for a good balance of speed and intelligence.

Open your terminal and run:

```bash
# Recommended for most users (Fast & Capable)
ollama pull glm-4.7-flash:latest

# Or try Meta's Llama 3 (8B)
ollama pull llama3
```

Other popular models:
-   `mistral`: Great general purpose.
-   `gemma`: Google's open model.
-   `codellama`: Specialized for coding tasks.

---

## 3. Use in Tandem

1.  Open **Tandem**.
2.  Click the **Model Selector** in the bottom chat bar (or go to Settings).
3.  You will see an **Ollama** section automatically populated with your installed models.
4.  Select a model and start chatting!

**Note:** If you install a new model while Tandem is open, just close and reopen the Model dropdown to refresh the list.
