# Veyra 取扱説明書 & カスタマイズ・完全マニュアル

<p align="center">
  <strong>日本語</strong> | <a href="./MANUAL.md">English</a>
</p>

Veyra（ヴェイラ）は、Windows OS 上で動作する **完全ローカル対応・常駐型自律 AI アシスタント＆タスクランタイム** です。  
美しいアンバーゴールドのフローティングカプセルバーと、必要に応じて展開できるプロ仕様のプロジェクトダッシュボード、そして Windows のファイル操作・プロセス実行・ブラウザ操作・ウィンドウ管理を行う「手足」を備えています。

---

## 目次
1. [基本操作 & ショートカット](#1-基本操作--ショートカット)
2. [AIモデル・APIの差し替えガイド（答えさせるAIを変える）](#2-aiモデルapiの差し替えガイド答えさせるaiを変える)
   - [A. ローカル Ollama モデルの変更](#a-ローカル-ollama-モデルの変更一番おすすめ)
   - [B. 外部 API（OpenAI / DeepSeek / Claude / Groq）を使う](#b-外部-apiopenai--deepseek--claude--groqを使う)
   - [C. LM Studio / vLLM / ローカルサーバーと接続する](#c-lm-studio--vllm--ローカルサーバーと接続する)
3. [音声の差し替えガイド（声や音声認識を変える）](#3-音声の差し替えガイド声や音声認識を変える)
   - [A. Style-Bert-VITS2 のモデル・キャラクター変更](#a-style-bert-vits2-のモデルキャラクター変更)
   - [B. VOICEVOX や他エンジンとの連携](#b-voicevox-や他エンジンとの連携)
   - [C. 音声認識（Whisper）モデルの変更](#c-音声認識whisperモデルの変更)
4. [自作 Windows ツールの追加方法](#4-自作-windows-ツールの追加方法)
5. [トラブルシューティング](#5-トラブルシューティング)

---

## 1. 基本操作 & ショートカット

Veyra はシステムトレイに常駐し、いつでもキーボードから呼び出せます。

| キー操作 | 動作 |
|---|---|
| **`Ctrl + Shift + Space`** | **Veyra の呼び出し / 格納**（画面上部中央からスライド表示） |
| **`Ctrl + D`** | **プロジェクトダッシュボードの開閉**（Goals, Tools, Verification, Settings） |
| **`Ctrl + Space`（長押し）** | **音声入力**（押している間マイク録音、離すと自動送信・認識） |
| **`Esc`** | **閉じる**（ダッシュボードが開いている時はダッシュボードを、HUDのみの時は全体を格納） |
| **`Enter`** | 入力テキストの送信 / アクション実行 |

### 画面の見方
1. **Pill HUD（カプセルバー）**:
   - **左端のオーブ（SiriOrb）**: AI の思考状態（待機: 青紫・明滅 / 録音: 赤・波紋 / 思考中: 回転 / 発話: 金色）。クリックで音声認識の開始/停止。
   - **入力欄**: 指示や質問を入力（例: `YouTubeを開いて`, `メモ帳を起動して`, `Agentの進捗を確認して`）。
   - **右端のボタン群**:
     - `↑`（送信）: テキスト入力時に出現
     - `🎤`（マイク）: 音声入力
     - `📊`（ダッシュボード）: 詳細ドロワーのトグル（`Ctrl+D` と同じ）
     - `プロジェクト選択`: 対象プロジェクトの切り替え
     - `🔊/🔇`: 自動音声読み上げのオン/オフ
     - `✕`: 格納

2. **ダッシュボード（`Ctrl + D`）の 4大タブ**:
   - 🎯 **Goals & Agent**: プロジェクトの目標、要件、受入基準（AC）、自律エージェント実行、プロジェクト直結チャット
   - ⚡ **Tools & Automation**: 利用可能な Windows ツール一覧、ワンタップ自動化タスク
   - 📋 **Verification & Decisions**: プロジェクト整合性検証（Plan §19）、決定事項、ユーザーフィードバック
   - ⚙️ **Voice & Settings**: 音声エンジン URL、モデル名、Whisper 設定

---

## 2. AIモデル・APIの差し替えガイド（答えさせるAIを変える）

Veyra は **「LLM ≠ Runtime（モデルは交換可能）」** という原則で設計されており、特定のモデルに縛られません。

### A. ローカル Ollama モデルの変更（一番おすすめ）
デフォルトでは以下のモデルが設定されています：
- **日常対話・意図判定（高速 1.5B）**: `qwen2.5:1.5b`
- **コーディング・自律Agent・検証（高精度 7B）**: `qwen2.5-coder:7b`

#### 別のモデルに変更したい場合:
1. Ollama で使いたいモデルを取得します:
   ```bash
   ollama pull deepseek-r1:8b
   ollama pull qwen2.5-coder:14b
   ```
2. **変更場所**: [`crates/veyra-runtime/src/text_loop.rs`](./crates/veyra-runtime/src/text_loop.rs)
   ```rust
   pub const INTENT_MODEL: &str = "qwen2.5:1.5b";      // 例: "llama3.2:3b"
   pub const CODING_MODEL: &str = "qwen2.5-coder:7b"; // 例: "deepseek-r1:8b" や "qwen2.5-coder:14b"
   ```
3. 再ビルドすれば即座に反映されます。

---

### B. 外部 API（OpenAI / DeepSeek / Claude / Groq）を使う
「PC の GPU が足りない」「超高精度な DeepSeek や GPT-4o を使いたい」という場合、**コードを変更することなく環境変数だけで接続できます**。

#### 設定方法（PowerShell の場合）:
```powershell
# DeepSeek API を使う例
$env:OPENAI_API_KEY = "sk-xxxxxxxxxxxxxxxxxxxxxxxx"
$env:OPENAI_BASE_URL = "https://api.deepseek.com/v1"
$env:OPENAI_MODEL = "deepseek-chat" # または deepseek-coder

# OpenAI 本家を使う例
$env:OPENAI_API_KEY = "sk-proj-xxxxxxxxxxxxxxxxxxxx"
$env:OPENAI_MODEL = "gpt-4o-mini" # または gpt-4o

# Groq（超高速 Llama 3）を使う例
$env:OPENAI_API_KEY = "gsk_xxxxxxxxxxxxxxxxxxxxxxx"
$env:OPENAI_BASE_URL = "https://api.groq.com/openai/v1"
$env:OPENAI_MODEL = "llama-3.3-70b-versatile"
```
これらを設定した状態で Veyra を起動すると、自動的に `OpenAICompatibleProvider` がルーターに登録され、外部 API 経由で推論が実行されます。

---

### C. LM Studio / vLLM / ローカルサーバーと接続する
LM Studio や vLLM などのローカル推論 GUI を使いたい場合も、OpenAI 互換エンドポイントとして接続できます：
```powershell
$env:OPENAI_API_KEY = "not-needed"
$env:OPENAI_BASE_URL = "http://localhost:1234/v1"
$env:OPENAI_MODEL = "local-model"
```

---

## 3. 音声の差し替えガイド（声や音声認識を変える）

### A. Style-Bert-VITS2 のモデル・キャラクター変更
Veyra は日本語ローカル音声合成エンジン **Style-Bert-VITS2 (SBV2)** にネイティブ対応しています。

1. **GUI からの変更（推奨・即時反映）**:
   - `Ctrl + D` でダッシュボードを開き、**⚙️ Voice & Settings** タブをクリック。
   - **TTS モデル識別子**: 例 `jvnv-M1-jp` を、自分の SBV2 サーバーにロードしているモデル名（例: `your-character-name`）に書き換えるだけ！
   - **サーバーURL**: 別の PC や GPU サーバーで動かしている場合は `http://192.168.1.100:5000` などの IP アドレスに変更可能。

2. **SBV2 が起動していない場合**:
   - 自動的に Windows のネイティブ日本語音声合成（SAPI / Web Speech）にフォールバックして声を出します。音が途切れる心配はありません。

---

### B. VOICEVOX や他エンジンとの連携
もし「ずんだもん」や VOICEVOX の声にしたい場合：
- `crates/veyra-voice/src/tts.rs` の `synthesize` 関数で、VOICEVOX の `/audio_query` & `/synthesis`（ポート 50021）を叩くクライアントを1ブロック追加するだけで連携可能です。

---

### C. 音声認識（Whisper）モデルの変更
Veyra は 2段階のハイブリッド音声認識を搭載しています：
1. **Web Speech API**: 話している最中にブラウザのリアルタイム認識で入力欄に即座に文字が出現。
2. **ローカル Whisper**: ボタンを離した瞬間に、ローカルの Whisper C++ エンジンで正確な日本語に校正。

Whisper モデルはダッシュボードの **⚙️ Voice & Settings** タブからいつでも切り替えられます：
- `tiny` (推奨): 約 75MB。CPU でも瞬時に文字起こし完了（デフォルト）。
- `base`: 約 145MB。軽量かつ日常会話に十分な精度。
- `small`: 約 466MB。固有名詞や技術用語に強い。
- `medium`: 約 1.5GB。最高精度（GPU 推奨）。

初回選択時に Hugging Face から自動ダウンロードされ、`%LOCALAPPDATA%\veyra\models` にキャッシュされます。

---

## 4. 自作 Windows ツールの追加方法

Veyra のエージェントに「新しい Windows 操作」を教えるのはとても簡単です。

1. [`crates/veyra-tools/src/`](./crates/veyra-tools/src/) に新しいツールを作ります。
2. `Tool` トレイトを実装します:
   ```rust
   pub struct MyCustomTool;

   #[async_trait]
   impl Tool for MyCustomTool {
       fn name(&self) -> &str { "open_calculator" }
       fn description(&self) -> &str { "Windowsの電卓を起動する" }
       fn permission_level(&self) -> PermissionLevel { PermissionLevel::LowRisk }

       async fn execute(&self, _ctx: &ToolContext, _input: serde_json::Value) -> Result<ToolResult, ToolError> {
           std::process::Command::new("calc.exe").spawn()?;
           Ok(ToolResult {
               output: serde_json::json!({ "status": "launched" }),
               summary: "電卓を起動しました".into(),
           })
       }
   }
   ```
3. [`crates/veyra-tools/src/registry.rs`](./crates/veyra-tools/src/registry.rs) の `default_tool_registry()` に `reg.register(Box::new(MyCustomTool));` を足すだけで、AI が自動で認識して使いこなすようになります！

---

## 5. トラブルシューティング

- **Q. `Ctrl+Shift+Space` を押してもウィンドウが出ない**:
  - タスクバー右下のシステムトレイ（隠れているインジケーター）に Veyra アイコン（`V`）があるか確認してください。
  - アイコンを右クリックして「Veyra を表示」を押すか、ダブルクリックしてください。
- **Q. 音声読み上げが鳴らない**:
  - `Ctrl + D` の Settings で「回答を自動音声読み上げ」のチェックボックスが入っているか確認してください。
  - SBV2 を使う場合は、ローカルで `python app.py` が起動して `http://127.0.0.1:5000` で待ち受けているか確認してください。
- **Q. Ollama Offline と表示される**:
  - Ollama が起動しているか確認してください（ターミナルで `ollama list`）。
  - `ollama serve` を実行するか、Windows スタートメニューから Ollama を起動してください。
