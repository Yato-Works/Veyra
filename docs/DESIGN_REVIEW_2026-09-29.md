# Veyra 設計の穴 — 全件調査・最終確定レポート (2026-09-29)

**検証日時**: 2026-09-29 19:38〜19:52  
**対象**: 作業ツリー最新  
**検証結果**:
- `cargo test`: 全79件 pass（失敗0）
- `cargo check --workspace --all-targets`: クリーン（エラー0）
- `npx tsc --noEmit`: exit 0
- 指摘の再照合: 28項目 → 修正済 6 / 部分修正 5 / 撤回 1 / 未対応 16 / 新規 1（R-1: 修正済）

---

## 判定マトリクス

| 状態 | 項目 |
|---|---|
| ✅ **修正済 (7)** | **C-1**: モデルルーティング（`router.rs` 裸名/ollama-接頭辞一致 ＋ task_type による Smart Tiering）<br>**C-2**: 完了ゲート（plan_truncated フラグ・空チェック=Unverified・`agent.rs` 回帰テスト）<br>**H-1**: アシスタント多段実行（MAX_ASSISTANT_STEPS = 3 の有界ループ）<br>**H-5**: CSP に `media-src 'self' data: blob:;` 追加<br>**M-2**: 未知イベント型の許容（前方互換性）<br>**M-4**: GoalUpdated 書き手が status も出力<br>**R-1**: 再計画時の `last_failure` クリア＋E2Eリカバリ回帰テスト |
| 🟡 **部分修正 (5)** | **H-6**: 既定モデルが tiny に統一（残: `stt.rs:182` で毎回モデルロード）<br>**M-1**: agent は ToolCompleted に全文を書かなくなった（残: `agent.rs:263` の input 全文、assistant の output 全文）<br>**M-6**: `windows_subsystem` 有効化＋相対ログ（残: cwd依存）<br>**M-7**: ProjectView をダッシュボード統合（残: `WhisperHUD.tsx` は孤児）<br>**L-3**: summarize に注入ガード追加（残: agent の `last_failure` 経路） |
| ↩️ **撤回 (1)** | **M-8**:「設定の無言書換」→ 実際は既定値の変更で、保存値は尊重されていたため撤回。 |
| ❌ **未対応 (16)** | **C-3**: 権限（既定 HighRisk・承認フロー無し・denylist・fs/shell 二重基準）<br>**C-4**: RuntimeState 非永続・EventBus 未使用<br>**H-2**: let _events 破棄のまま<br>**H-3**: 未生成イベント型<br>**H-4**: feedback が Goal を変えず resume UI も無し<br>**H-7**: single-instance 無し（lib.rs:587 の expect）<br>**H-8**: 進捗・キャンセル無し<br>**M-3**: Projector 無言破棄<br>**M-5**: 権限UI無し<br>**M-9**: スキーマ版管理無し<br>**M-10**: 死にスキーマ・tool_runs 未使用<br>**L-1, L-2, L-4, L-5, L-6** |

---

## 修正詳細 & 新規回帰 (R-1) の解決

### R-1: 「再計画が成功しても完了できない」バグの解消
- **原因**: `agent.rs` の `last_failure` に `= None` へ戻す箇所が存在しなかったため、一度でも失敗すると REPLAN 後の計画が全ステップ成功しても `all_steps_ok` が永久に false になっていた。
- **対応**: `self.plan_steps` でプロンプトに直前の失敗を渡した直後、新しい計画実行前に `last_failure = None;` を追加。
- **検証**: `agent_recovers_via_replan_and_completes_r1` テストを追加し、失敗 → 再計画 → 検証 → Completed の一連のリカバリフローが正常に通過することを立証。
