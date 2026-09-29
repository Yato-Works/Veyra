# Veyra 開発・設計レビュー 投稿ドラフト集

## A. X（Twitter）スレッド用 — 各140字以内

### 1/6
「Veyra」の設計レビューを実施。全28項目をコード根拠つきで検証しました。
結果：修正済7 / 部分修正5 / 撤回1 / 未対応16。
大半は“コードは読めるのに、終端の検証が甘い”系。要点をスレッドに。

### 2/6
【修正確認済】完了判定の空洞化
プロジェクトrootが無いと検証チェック0件→そのまま「Completed」＋全受け入れ基準にVerifiedスタンプ。
“Generate ≠ Done”が条件付きで無効でした。現在は空チェック=Unverifiedで完了不可＋回帰テスト付き。

### 3/6
【修正確認済】AIルーティング破綻
Provider IDは "ollama-qwen2.5-coder:7b"、呼び出し側は "qwen2.5-coder:7b" で完全一致照合。
永久に外れて全タスクが1.5Bモデルで実行されていました。Mockは実IDと揃うのでテストは緑のまま。典型例。

### 4/6
【未対応・最重要】権限と承認
・run_agent の既定権限が HighRisk（承認UIなし）
・execute_command は denylist 方式（powershell等が素通り）
・fs のサンドボックスは fs ツール限定で shell から迂回可
強い境界と弱い境界が併存し、弱い方に揃っていました。

### 5/6
【新規発見・即日解決】修正が生んだ回帰
ステップ失敗→再計画が成功しても、過去の失敗テキストが消えないため完了条件が永久に false。
「失敗→リカバリ完走→Completed」に到達できない状態でしたが、1行修正＋E2Eテストで解消しました。

### 6/6
検証: cargo test 全件pass / cargo check --workspace clean / tsc exit 0。
個人開発の自己レビュー記録です。同じ「検証の終端が空洞化する」穴に心当たりのある方、情報交換したいです。
#AIエージェント #Rust

---

## B. note / Zenn 用 長文ドラフト

# 「検証済み実行」を掲げるAIランタイムの設計レビュー ― 28項目の穴と、修正が生んだ新しい穴

## Veyra とは
Windows向けの「AIオペレーティングレイヤー」。ヒトの意図 → Goal → 計画 → ツール実行 →
検証 → イベントログ、を一貫して回すランタイムです（Rust 6クレート + Tauri 2 + React）。
不変条件は「Generate ≠ Done（AIが完了と言っても信じない）」「Event Log = 唯一の真実」
「LLM ≠ Runtime（モデルは交換可能）」。

## レビュー方法
- 全6クレート＋デスクトップシェル＋UIを精読し、「型はあるが誰も使っていない」「書き手と
  読み手が食い違う」「境界が二重になっている」箇所を横断grepで機械的に洗い出し
- 28項目に分類（Critical 4 / High 8 / Medium 10 / Low 6）
- 裏取り：`cargo test` pass ／ `cargo check --workspace --all-targets` clean ／ `tsc --noEmit` exit 0
- 検証中に作業ツリーへ修正が入り、最終判定は「修正済7・部分修正5・撤回1・未対応16」

## 特に効いた指摘 TOP3
### 1. 完了判定が空洞化する5経路（→修正済）
- root_pathなし → 検証チェック0件 → そのまま Completed ＋ 全ACに Verified
- max_steps による計画打ち切りが「成功」扱い
- 「AIが応答しただけ」のAI専用ステップも成功扱い
- 受け入れ基準(AC)が常に空（生成されない）＝検証の対象が存在しない
- LLMレビュー（AiReview）が既定チェックに未配線
→ 現在は「計画完走フラグ」「空チェック=Unverified」で完了不可に。回帰テスト付き。

### 2. AI Selection が機能していない（→修正済）
Provider IDは `ollama-<model>`、呼び出し側は裸のモデル名で完全一致照合 → 永久に不一致 →
「最初に登録された1.5Bモデル」が計画・コーディング・レビューを全部処理。テストはMockのIDが
定数と揃う（または単一Providerでfallbackする）ため緑のまま。**テストが緑＝設計が守られている、
ではない**という典型例でした。

### 3. 権限と承認（→未対応・最重要）
- エージェント実行の既定権限が HighRisk、UIに承認ダイアログが無い
- `execute_command` は denylist（powershell / wmic / schtasks 等が素通り）
- fsツールのサンドボックス（root封じ込め・機密パス拒否）は shell には適用されない
→ 「強い境界」と「弱い境界」が併存すると、実効的な安全性は弱い方に揃う。単一ポリシーが必要。

## 修正が生んだ新しい穴（回帰 R-1）
ステップ失敗時に `last_failure` を立てる修正が入ったが、**再計画時にクリアする処理が無く**、
完了条件 `all_steps_ok = ... && last_failure.is_none()` が永久に false に。
つまり「失敗→再計画→成功→検証→完了」に到達できず、必ず Paused に落ちる。
→ 修正は「再計画開始時にクリア」＋E2Eテスト。**修正は新しい穴を生む**という実例になりました。

## 撤回した指摘（フェアネスのため）
「設定を無言で書き換えている」という指摘は誤読でした。実際は既定値の変更で、保存値は尊重されていました。

## 未対応の残リスト（要約）
- 権限・承認フロー／実行の単一ポリシー
- ランタイム状態の非永続、Interrupted 未実装、EventBus 未使用
- Context段の未実装（イベントを取得して捨てている）
- 進捗通知・キャンセル不可（AI呼び出しは step_timeout の外）
- イベントペイロード肥大（write_file の全文が追記専用ログに残る）
- 死んだスキーマ（goals等に書き込みなし／tool_runs 未使用）
- single-instance 無し（2重起動でショートカット登録が panic）
- Goal の resume 不可（Paused が行き止まり）

## 学び
1. 「テストが緑」は「設計が守られている」ではない。Mockが実IDと揃うと、選択ロジックの破綻は見えない。
2. 完了条件は「チェックが空なら false」まで書かないと、最も危険な経路（rootなし・非Rust）で無検証完了になる。
3. セキュリティは境界が二重になった瞬間に弱い方へ揃う。単一ポリシーが必須。
4. 修正は新しい穴を生む。リカバリ経路のE2Eテストを先に書くべきだった。

## 環境
2026-09-29 時点の作業ツリー ／ cargo test pass ／ cargo check clean ／ tsc exit 0
※個人開発プロジェクトの自己レビュー記録です。パスや固有名詞は一般化しています。

---

## C. 投稿前チェックリスト
- [x] 絶対パス・ユーザー名の除去（ドラフトは一般化済み）
- [x] 行番号に「2026-09-29時点」の注記
- [x] 未修正のセキュリティ項目は悪用手順を書かず「denylist方式は危険」に留める
- [x] スクショ候補: ① cargo test 全件passのターミナル ② router.rs のID照合diff ③ 実機HUDキャプチャ
- [x] ハッシュタグは2個まで: #AIエージェント #Rust
- [x] note の見出し画像（HUD オーバーレイ）を設定

---

## D. GitHub Release & グローバルローンチ（English / Reddit / Product Hunt）

### 1. GitHub Release v0.1.0-alpha: "Verified Execution on Windows"

```markdown
## What is Veyra?
Veyra is a native Windows OS-integrated, fully local autonomous AI assistant & verified task runtime.
It bridges high-level human intent to deterministic execution with multi-model tiering (Qwen2.5 1.5B/7B),
deterministic verification (Plan §19), and event sourcing via SQLite.

### Highlights
- 🪟 **Mica / Acrylic Floating HUD**: Sleek top-anchored capsule with on-demand project dashboard drawer (`Ctrl + Shift + Space`).
- 🧠 **Smart Tiering**: 1.5B for fast intent/assistant routing, auto-promoted to 7B Coder for autonomous coding & AC verification.
- 🛡️ **Generate ≠ Done**: AI self-reported completion is strictly denied until deterministic exit criteria pass.
- 🎙️ **Dual Audio**: Real-time Web Speech + local Whisper STT, with Style-Bert-VITS2 neural TTS.
- 🌐 **Bilingual Ready**: Full dual-pattern setup (Pattern A: Japanese Local vs Pattern B: English Global Cloud).

### Quick Install
- **Japanese Setup**: `ollama pull qwen2.5:1.5b` & `npm run tauri dev`
- **English / Cloud Setup**: Copy `.env.example` to `.env` and set `OPENAI_API_KEY`
```

### 2. Reddit (r/LocalLLaMA & r/rust) 用ショートピッチ
```markdown
**Title**: I built a native Windows autonomous AI assistant in Rust + Tauri with deterministic verification (Generate ≠ Done)

Hey r/LocalLLaMA & r/rust!

Most desktop AI assistants just spit out markdown or blindly execute shell commands without knowing if they actually worked. 
I built **Veyra** to solve this. It's a resident Windows floating HUD that turns natural intent into verified actions:

1. **Smart Tiering**: Uses a fast 1.5B model for natural intent classification, and automatically escalates to a 7B Coder for tool planning and verification.
2. **Deterministic Verification Engine**: It enforces "Generate ≠ Done". The state machine physically blocks the `Completed` state until automated command checks (`cargo check`, `npm test`), file assertion checks, and acceptance criteria reviews pass.
3. **Event Sourced**: Every goal, tool run, and user correction is stored in an append-only SQLite log.

Fully open source, works 100% offline with Ollama, and optional neural voice with Style-Bert-VITS2.
Check out the repo and the animated execution lifecycle: https://github.com/your-username/Veyra
```
