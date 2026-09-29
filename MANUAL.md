# Veyra Comprehensive Manual & Customization Guide

<p align="center">
  <a href="./MANUAL.ja.md">日本語 (Japanese)</a> | <strong>English</strong>
</p>

Veyra is a native Windows **fully-local autonomous AI assistant & task runtime**.  
It features an amber-gold floating capsule HUD, an on-demand project dashboard drawer, and direct Windows execution primitives (process management, filesystem I/O, browser automation, and window control).

---

## Table of Contents
1. [Basic Controls & Shortcuts](#1-basic-controls--shortcuts)
2. [AI Model & API Customization Guide](#2-ai-model--api-customization-guide)
   - [A. Changing Local Ollama Models](#a-changing-local-ollama-models-recommended)
   - [B. Using External Cloud APIs (OpenAI / DeepSeek / Claude / Groq)](#b-using-external-cloud-apis-openai--deepseek--claude--groq)
   - [C. Connecting to LM Studio / vLLM / Local Inference Servers](#c-connecting-to-lm-studio--vllm--local-inference-servers)
3. [Audio Customization Guide](#3-audio-customization-guide)
   - [A. Style-Bert-VITS2 Voice Model Configuration](#a-style-bert-vits2-voice-model-configuration)
   - [B. Fallback and Alternative TTS Engines](#b-fallback-and-alternative-tts-engines)
   - [C. Local Whisper STT Model Selection](#c-local-whisper-stt-model-selection)
4. [Creating Custom Windows Tools](#4-creating-custom-windows-tools)
5. [Troubleshooting & FAQ](#5-troubleshooting--faq)

---

## 1. Basic Controls & Shortcuts

Veyra resides silently in the Windows system tray and can be summoned instantly via keyboard shortcuts.

| Hotkey | Action | Notes |
|---|---|---|
| **`Ctrl + Shift + Space`** | **Toggle Capsule HUD** | Slides down smoothly from the top center of the screen |
| **`Ctrl + D`** | **Toggle Project Dashboard Drawer** | Opens Goals, Tools, Verification, and Settings tabs |
| **`Ctrl + Space` (Hold)** | **Push-to-Talk Voice Input** | Records audio while held; transcribes upon release |
| **`Esc`** | **Dismiss / Close** | Closes drawer if open; hides HUD if drawer is closed |
| **`Enter`** | **Submit / Execute** | Confirms text input |

### UI Overview
1. **Capsule HUD**:
   - **Left SiriOrb**: Visual indicator of AI cognitive state (Idle: pulsing indigo / Listening: red ripple / Thinking: rotating / Speaking: amber glow). Click to manually toggle voice recognition.
   - **Command Input**: Type natural-language requests (e.g., `Open YouTube`, `Start Notepad`, `Check autonomous agent progress`).
   - **Action Buttons**:
     - `↑`: Submit text command
     - `🎤`: Voice input toggle
     - `📊`: Dashboard drawer toggle (`Ctrl + D`)
     - `Project Selector`: Switch active workspace / project
     - `🔊/🔇`: Audio TTS toggle
     - `✕`: Dismiss to tray

2. **Dashboard Drawer (`Ctrl + D`) 4 Core Tabs**:
   - 🎯 **Goals & Agent**: Define project goals, inspect requirements & Acceptance Criteria (AC), monitor autonomous agent step execution.
   - ⚡ **Tools & Automation**: Browse registered Windows automation tools and execute one-click tasks.
   - 📋 **Verification & Decisions**: Deterministic verification checks (Plan §19), architectural decision records, and audit logs.
   - ⚙️ **Voice & Settings**: TTS server endpoints, voice model IDs, and Whisper configuration.

---

## 2. AI Model & API Customization Guide

Veyra adheres strictly to the principle **"LLM ≠ Runtime (Models are Pluggable)"**. You can swap models without altering runtime state guarantees.

### A. Changing Local Ollama Models (Recommended)
Default configuration:
- **Intent Detection & Quick Assistant (Fast 1.5B)**: `qwen2.5:1.5b`
- **Coding, Autonomous Planning & Verification (High-Capacity 7B)**: `qwen2.5-coder:7b`

#### To use alternative models:
1. Pull desired models in Ollama:
   ```bash
   ollama pull deepseek-r1:8b
   ollama pull qwen2.5-coder:14b
   ```
2. **Edit config**: [`crates/veyra-runtime/src/text_loop.rs`](./crates/veyra-runtime/src/text_loop.rs)
   ```rust
   pub const INTENT_MODEL: &str = "qwen2.5:1.5b";      // e.g., "llama3.2:3b"
   pub const CODING_MODEL: &str = "qwen2.5-coder:7b"; // e.g., "deepseek-r1:8b" or "qwen2.5-coder:14b"
   ```
3. Rebuild to apply changes.

---

### B. Using External Cloud APIs (OpenAI / DeepSeek / Claude / Groq)
If local GPU memory is limited or you prefer high-parameter models, configure standard environment variables without modifying source code:

#### Configuration (PowerShell):
```powershell
# Example: DeepSeek API
$env:OPENAI_API_KEY = "sk-xxxxxxxxxxxxxxxxxxxxxxxx"
$env:OPENAI_BASE_URL = "https://api.deepseek.com/v1"
$env:OPENAI_MODEL = "deepseek-chat" # or deepseek-coder

# Example: OpenAI Official
$env:OPENAI_API_KEY = "sk-proj-xxxxxxxxxxxxxxxxxxxx"
$env:OPENAI_MODEL = "gpt-4o-mini" # or gpt-4o

# Example: Groq (Ultra-low latency Llama 3)
$env:OPENAI_API_KEY = "gsk_xxxxxxxxxxxxxxxxxxxxxxx"
$env:OPENAI_BASE_URL = "https://api.groq.com/openai/v1"
$env:OPENAI_MODEL = "llama-3.3-70b-versatile"
```
When Veyra detects `OPENAI_API_KEY`, it automatically registers `OpenAICompatibleProvider` into the runtime router.

---

### C. Connecting to LM Studio / vLLM / Local Inference Servers
Local inference GUI servers exposing OpenAI-compatible endpoints can be connected identically:
```powershell
$env:OPENAI_API_KEY = "not-needed"
$env:OPENAI_BASE_URL = "http://localhost:1234/v1"
$env:OPENAI_MODEL = "local-model"
```

---

## 3. Audio Customization Guide

### A. Style-Bert-VITS2 Voice Model Configuration
Veyra provides native integration with **Style-Bert-VITS2 (SBV2)**.

1. **Configuring via Dashboard (Immediate)**:
   - Press `Ctrl + D` and open the **⚙️ Voice & Settings** tab.
   - **TTS Model Identifier**: Change `jvnv-M1-jp` to your server's loaded model name.
   - **Server Endpoint**: If running on a dedicated GPU host on your LAN, update the URL (e.g., `http://192.168.1.100:5000`).

2. **Graceful Fallback**:
   - If SBV2 is unreachable, Veyra seamlessly falls back to Windows native SAPI speech synthesis.

---

### B. Fallback and Alternative TTS Engines
Alternative TTS engines (such as VOICEVOX or custom HTTP synthesis endpoints) can be integrated by extending the `synthesize` implementation in `crates/veyra-voice/src/tts.rs`.

---

### C. Local Whisper STT Model Selection
Veyra implements a two-phase hybrid speech recognition pipeline:
1. **Web Speech API**: In-browser real-time transcription populates the input field dynamically while speaking.
2. **Local Whisper**: Upon releasing the hotkey, the audio stream is passed to the embedded `whisper.cpp` engine for high-accuracy phonetic correction.

Whisper models can be switched on the fly in **⚙️ Voice & Settings**:
- `tiny` (Default / Recommended): ~75MB, instantaneous on CPU.
- `base`: ~145MB, balanced speed and daily conversation accuracy.
- `small`: ~466MB, strong recognition for technical jargon and proper nouns.
- `medium`: ~1.5GB, highest accuracy (GPU recommended).

---

## 4. Creating Custom Windows Tools

Extending Veyra's autonomous agent with custom Windows capabilities takes just a few lines of Rust:

1. Create a new tool file in [`crates/veyra-tools/src/`](./crates/veyra-tools/src/).
2. Implement the `Tool` trait:
   ```rust
   pub struct MyCustomTool;

   #[async_trait]
   impl Tool for MyCustomTool {
       fn name(&self) -> &str { "open_calculator" }
       fn description(&self) -> &str { "Launch Windows native calculator" }
       fn permission_level(&self) -> PermissionLevel { PermissionLevel::LowRisk }

       async fn execute(&self, _ctx: &ToolContext, _input: serde_json::Value) -> Result<ToolResult, ToolError> {
           std::process::Command::new("calc.exe").spawn()?;
           Ok(ToolResult {
               output: serde_json::json!({ "status": "launched" }),
               summary: "Launched calculator successfully".into(),
           })
       }
   }
   ```
3. Register the tool in `default_tool_registry()` inside [`crates/veyra-tools/src/registry.rs`](./crates/veyra-tools/src/registry.rs):
   ```rust
   reg.register(Box::new(MyCustomTool));
   ```
   The autonomous agent will immediately discover and utilize your tool.

---

## 5. Troubleshooting & FAQ

- **Q: HUD does not appear when pressing `Ctrl + Shift + Space`**:
  - Check the Windows system tray notification area (overflow chevron `^`) for the Veyra icon (`V`).
  - Right-click the icon and choose "Show Veyra", or double-click to restore.
- **Q: TTS audio does not play**:
  - Open `Ctrl + D` Settings and verify that "Auto-play TTS" is enabled.
  - If using SBV2, confirm your FastAPI server is running on `http://127.0.0.1:5000`.
- **Q: Status reports "Ollama Offline"**:
  - Verify Ollama is running (`ollama list` in terminal). Start it via `ollama serve` or from the Windows Start menu.
