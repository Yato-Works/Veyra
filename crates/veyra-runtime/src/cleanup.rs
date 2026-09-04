//! Natural language cleanup for Flow Mode transcripts (Plan.txt section 9).
//!
//! Input:
//! > 「えーっと今日のミーティングなんですけどまぁ来週までには終わらせたいと思っていて」
//!
//! Output:
//! > 「今日のミーティングについてですが、来週までには終わらせたいと考えています。」
//!
//! Rules: remove fillers and accidental repetition, preserve meaning and
//! technical terms, never invent information, never answer the user, never
//! summarize. Preferably local and low-latency (qwen2.5:1.5b). On any AI
//! failure the raw transcript is returned unchanged — cleanup is an
//! enhancement, not a gate.

use serde_json::json;
use veyra_ai::{complete_with_fallback, AIRequest, AIRouter, Capabilities};

use crate::text_loop::INTENT_MODEL;

/// Clean up a raw STT transcript. Falls back to the raw text on failure.
pub async fn cleanup_transcript(router: &AIRouter, raw: &str) -> String {
    if raw.trim().is_empty() {
        return raw.to_string();
    }

    let request = AIRequest {
        task_type: "cleanup".into(),
        prompt: format!(
            "次の音声書き起こしテキストを整えてください。\n\
             ルール:\n\
             - フィラー(えー、あのー、えーっと等)と無意味な繰り返しを除去する\n\
             - 意味・専門用語・話者の意図は保持する\n\
             - 情報を付け加えない\n\
             - 質問に答えない\n\
             - 要約しない\n\n\
             入力: {raw}\n\n\
             整形後のテキストのみを出力してください(説明は不要)。"
        ),
        context: json!({}),
        max_tokens: Some(500),
        temperature: Some(0.2),
    };

    match complete_with_fallback(router, request, Some(INTENT_MODEL), &Capabilities::default())
        .await
    {
        Ok(resp) => {
            let cleaned = resp.text.trim();
            if cleaned.is_empty() {
                raw.to_string()
            } else {
                cleaned
                    .trim_matches(|c| c == '"' || c == '「' || c == '」')
                    .to_string()
            }
        }
        Err(_) => raw.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use veyra_ai::MockProvider;

    #[tokio::test]
    async fn cleanup_uses_ai_reply() {
        let mut router = AIRouter::new();
        router.register(Arc::new(MockProvider::new(
            INTENT_MODEL,
            "今日のミーティングについてですが、来週までには終わらせたいと考えています。",
        )));
        let out = cleanup_transcript(&router, "えーっと今日のミーティングなんですけど").await;
        assert_eq!(out, "今日のミーティングについてですが、来週までには終わらせたいと考えています。");
    }

    #[tokio::test]
    async fn cleanup_falls_back_to_raw_on_ai_failure() {
        let router = AIRouter::new();
        let raw = "えーっとそのまま返してほしいテキスト";
        let out = cleanup_transcript(&router, raw).await;
        assert_eq!(out, raw);
    }

    #[tokio::test]
    async fn empty_input_short_circuits() {
        let router = AIRouter::new();
        assert_eq!(cleanup_transcript(&router, "   ").await, "   ");
    }
}
