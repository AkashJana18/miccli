pub mod llm;
pub mod rules;

use crate::config::LlmConfig;

pub async fn cleanup(text: &str, llm_config: &LlmConfig) -> String {
    // Tier 1: Always apply regex rules
    let mut result = rules::apply(text);

    // Tier 2: LLM cleanup for complex text
    if llm_config.enabled && result.len() > 20 {
        if let Some(cleaned) = llm::llm_cleanup(&result, llm_config).await {
            result = cleaned;
        }
    }

    result
}
