use crate::session::Session;

pub use code_types::{ModelPricing, TokenUsage, UsageCostEstimate};

// Returns pricing metadata for a known model alias or family.
//
// Pricing reflects published list prices as of October 2026. `Anthropic`,
// `OpenAI`, and `xAI` list prices are used; unknown families return `None`
// so callers fall back to the default Sonnet-tier estimate.
#[must_use]
pub fn pricing_for_model(model: &str) -> Option<ModelPricing> {
    let normalized = model.to_ascii_lowercase();
    if normalized.contains("opus")
        || normalized.contains("sonnet")
        || normalized.contains("haiku")
        || normalized.contains("fable")
        || normalized.contains("mythos")
        || normalized.contains("claude")
    {
        return anthropic_pricing_for_model(&normalized);
    }
    if normalized.contains("gpt") {
        return openai_pricing_for_model(&normalized);
    }
    if normalized.contains("grok") {
        return xai_pricing_for_model(&normalized);
    }
    None
}

fn anthropic_pricing_for_model(normalized: &str) -> Option<ModelPricing> {
    // Most specific matches first: a bare "claude-opus-5" must not match the
    // cheaper "opus-5-5" tier, and "haiku-5-5" must not match legacy "haiku".
    if normalized.contains("fable") || normalized.contains("mythos") {
        // Claude Fable/Mythos 5.1: $10/$50 per million.
        return Some(ModelPricing {
            input_cost_per_million: 10.0,
            output_cost_per_million: 50.0,
            cache_creation_cost_per_million: 12.5,
            cache_read_cost_per_million: 0.25,
        });
    }
    if normalized.contains("haiku-5-5") || normalized == "haiku" {
        // Claude Haiku 5.5 base tier (prompts <=100K): $0.10/$0.50 per
        // million. Prompts above 100K tokens bill at $0.50/$2.50.
        return Some(ModelPricing {
            input_cost_per_million: 0.1,
            output_cost_per_million: 0.5,
            cache_creation_cost_per_million: 0.125,
            cache_read_cost_per_million: 0.01,
        });
    }
    if normalized.contains("haiku") {
        // Legacy Claude Haiku 4.5: $1/$5 per million.
        return Some(ModelPricing {
            input_cost_per_million: 1.0,
            output_cost_per_million: 5.0,
            cache_creation_cost_per_million: 1.25,
            cache_read_cost_per_million: 0.1,
        });
    }
    if normalized.contains("opus-5-5") || normalized.contains("opus-5.5") {
        // Claude Opus 5.5: $4/$20 per million.
        return Some(ModelPricing {
            input_cost_per_million: 4.0,
            output_cost_per_million: 20.0,
            cache_creation_cost_per_million: 5.0,
            cache_read_cost_per_million: 0.2,
        });
    }
    if normalized.contains("opus") {
        // Opus list pricing since the 4.x generation: $5/$25 per million.
        return Some(ModelPricing {
            input_cost_per_million: 5.0,
            output_cost_per_million: 25.0,
            cache_creation_cost_per_million: 6.25,
            cache_read_cost_per_million: 0.5,
        });
    }
    if normalized.contains("sonnet-5") {
        // Claude Sonnet 5 / 5.5: $2/$10 per million.
        return Some(ModelPricing {
            input_cost_per_million: 2.0,
            output_cost_per_million: 10.0,
            cache_creation_cost_per_million: 2.5,
            cache_read_cost_per_million: 0.1,
        });
    }
    if normalized.contains("sonnet") {
        return Some(ModelPricing::default_sonnet_tier());
    }
    None
}

fn openai_pricing_for_model(normalized: &str) -> Option<ModelPricing> {
    // Short-context standard tier rates.
    if normalized.contains("gpt-6-astra") {
        return Some(pricing(10.0, 50.0, 1.0));
    }
    if normalized.contains("gpt-6.1-sol") || normalized.contains("gpt-6-1-sol") {
        return Some(pricing(2.0, 10.0, 0.1));
    }
    if normalized.contains("gpt-6-luna") || normalized.contains("gpt-6.1-luna") {
        return Some(pricing(0.1, 0.5, 0.01));
    }
    if normalized.contains("gpt-5.6-sol") {
        return Some(pricing(4.0, 20.0, 0.4));
    }
    if normalized.contains("gpt-5.6-terra") {
        return Some(pricing(2.0, 12.0, 0.2));
    }
    if normalized.contains("gpt-5.6-luna") {
        return Some(pricing(0.2, 1.2, 0.02));
    }
    if normalized.contains("gpt-5.5") {
        return Some(pricing(5.0, 30.0, 0.5));
    }
    if normalized.contains("5.3-codex") {
        return Some(pricing(1.75, 14.0, 0.175));
    }
    if normalized.contains("gpt-5-mini") || normalized.contains("gpt-5.4-mini") {
        return Some(pricing(0.25, 2.0, 0.025));
    }
    if normalized.contains("gpt-5-nano") || normalized.contains("gpt-5.4-nano") {
        return Some(pricing(0.05, 0.4, 0.005));
    }
    if normalized.contains("gpt-5.1")
        || (normalized.contains("gpt-5") && !normalized.contains("gpt-5."))
    {
        return Some(pricing(1.25, 10.0, 0.125));
    }
    if normalized.contains("gpt-4.1-mini") {
        return Some(pricing(0.4, 1.6, 0.1));
    }
    if normalized.contains("gpt-4.1") {
        return Some(pricing(2.0, 8.0, 0.5));
    }
    // Unknown GPT variant: no verified rate, so let callers flag the
    // Sonnet-tier fallback explicitly.
    None
}

fn xai_pricing_for_model(normalized: &str) -> Option<ModelPricing> {
    // Short-context tier (prompts <200K tokens).
    if normalized.contains("grok-4.7") || normalized.contains("grok-4.6") || normalized == "grok" {
        return Some(pricing(2.0, 6.0, 0.5));
    }
    if normalized.contains("grok-4.5") {
        return Some(pricing(2.0, 6.0, 0.3));
    }
    if normalized.contains("grok-4.3") || normalized == "grok-mini" {
        return Some(pricing(1.25, 2.5, 0.2));
    }
    if normalized.contains("grok-build") || normalized.contains("grok-code") {
        return Some(pricing(1.0, 2.0, 0.2));
    }
    // Unknown Grok variant: no verified rate.
    None
}

const fn pricing(
    input_per_million: f64,
    output_per_million: f64,
    cached_input_per_million: f64,
) -> ModelPricing {
    // Neither `OpenAI` nor `xAI` charges a separate cache-write rate, so
    // writes are estimated at the base input rate.
    ModelPricing {
        input_cost_per_million: input_per_million,
        output_cost_per_million: output_per_million,
        cache_creation_cost_per_million: input_per_million,
        cache_read_cost_per_million: cached_input_per_million,
    }
}

#[must_use]
/// Formats a dollar-denominated value for CLI display.
pub fn format_usd(amount: f64) -> String {
    format!("${amount:.4}")
}

/// Aggregates token usage across a running session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsageTracker {
    latest_turn: TokenUsage,
    cumulative: TokenUsage,
    turns: u32,
}

impl UsageTracker {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn from_session(session: &Session) -> Self {
        let mut tracker = Self::new();
        for message in &session.messages {
            if let Some(usage) = message.usage {
                tracker.record(usage);
            }
        }
        tracker
    }

    pub fn record(&mut self, usage: TokenUsage) {
        self.latest_turn = usage;
        self.cumulative.input_tokens += usage.input_tokens;
        self.cumulative.output_tokens += usage.output_tokens;
        self.cumulative.cache_creation_input_tokens += usage.cache_creation_input_tokens;
        self.cumulative.cache_read_input_tokens += usage.cache_read_input_tokens;
        self.turns += 1;
    }

    #[must_use]
    pub fn current_turn_usage(&self) -> TokenUsage {
        self.latest_turn
    }

    #[must_use]
    pub fn cumulative_usage(&self) -> TokenUsage {
        self.cumulative
    }

    #[must_use]
    pub fn turns(&self) -> u32 {
        self.turns
    }
}

#[cfg(test)]
mod tests {
    use super::{format_usd, pricing_for_model, TokenUsage, UsageTracker};
    use crate::session::{ContentBlock, ConversationMessage, MessageRole, Session};

    #[test]
    fn tracks_true_cumulative_usage() {
        let mut tracker = UsageTracker::new();
        tracker.record(TokenUsage {
            input_tokens: 10,
            output_tokens: 4,
            cache_creation_input_tokens: 2,
            cache_read_input_tokens: 1,
        });
        tracker.record(TokenUsage {
            input_tokens: 20,
            output_tokens: 6,
            cache_creation_input_tokens: 3,
            cache_read_input_tokens: 2,
        });

        assert_eq!(tracker.turns(), 2);
        assert_eq!(tracker.current_turn_usage().input_tokens, 20);
        assert_eq!(tracker.current_turn_usage().output_tokens, 6);
        assert_eq!(tracker.cumulative_usage().output_tokens, 10);
        assert_eq!(tracker.cumulative_usage().input_tokens, 30);
        assert_eq!(tracker.cumulative_usage().total_tokens(), 48);
    }

    #[test]
    fn computes_cost_summary_lines() {
        let usage = TokenUsage {
            input_tokens: 1_000_000,
            output_tokens: 500_000,
            cache_creation_input_tokens: 100_000,
            cache_read_input_tokens: 200_000,
        };

        let cost = usage.estimate_cost_usd();
        assert_eq!(format_usd(cost.input_cost_usd), "$3.0000");
        assert_eq!(format_usd(cost.output_cost_usd), "$7.5000");
        let pricing = pricing_for_model("claude-sonnet-4-20250514").expect("sonnet pricing");
        let lines = usage.summary_lines_for_model("usage", Some(&pricing));
        assert!(lines[0].contains("estimated_cost=$10.9350"));
        assert!(lines[1].contains("cache_read=$0.0600"));
    }

    #[test]
    fn supports_model_specific_pricing() {
        let usage = TokenUsage {
            input_tokens: 1_000_000,
            output_tokens: 500_000,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
        };

        let haiku = pricing_for_model("claude-haiku-4-5-20251001").expect("haiku pricing");
        let opus = pricing_for_model("claude-opus-5").expect("opus pricing");
        let haiku_cost = usage.estimate_cost_usd_with_pricing(haiku);
        let opus_cost = usage.estimate_cost_usd_with_pricing(opus);
        assert_eq!(format_usd(haiku_cost.total_cost_usd()), "$3.5000");
        assert_eq!(format_usd(opus_cost.total_cost_usd()), "$17.5000");
    }

    #[test]
    fn supports_current_generation_pricing() {
        let usage = TokenUsage {
            input_tokens: 1_000_000,
            output_tokens: 1_000_000,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
        };

        let opus = pricing_for_model("claude-opus-5-5").expect("opus 5.5 pricing");
        assert_eq!(
            format_usd(usage.estimate_cost_usd_with_pricing(opus).total_cost_usd()),
            "$24.0000"
        );
        let sonnet = pricing_for_model("claude-sonnet-5-5").expect("sonnet 5.5 pricing");
        assert_eq!(
            format_usd(
                usage
                    .estimate_cost_usd_with_pricing(sonnet)
                    .total_cost_usd()
            ),
            "$12.0000"
        );
        let haiku = pricing_for_model("haiku").expect("haiku alias pricing");
        assert_eq!(
            format_usd(usage.estimate_cost_usd_with_pricing(haiku).total_cost_usd()),
            "$0.6000"
        );
        let fable = pricing_for_model("claude-fable-5-1").expect("fable pricing");
        assert_eq!(
            format_usd(usage.estimate_cost_usd_with_pricing(fable).total_cost_usd()),
            "$60.0000"
        );
        let codex = pricing_for_model("gpt-5.3-codex").expect("codex pricing");
        assert_eq!(
            format_usd(usage.estimate_cost_usd_with_pricing(codex).total_cost_usd()),
            "$15.7500"
        );
        let grok = pricing_for_model("grok-4.7").expect("grok pricing");
        assert_eq!(
            format_usd(usage.estimate_cost_usd_with_pricing(grok).total_cost_usd()),
            "$8.0000"
        );
        let grok_code = pricing_for_model("grok-build-0.1").expect("grok code pricing");
        assert_eq!(
            format_usd(
                usage
                    .estimate_cost_usd_with_pricing(grok_code)
                    .total_cost_usd()
            ),
            "$3.0000"
        );
    }

    #[test]
    fn marks_unknown_model_pricing_as_fallback() {
        let usage = TokenUsage {
            input_tokens: 100,
            output_tokens: 100,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
        };
        let lines = usage.summary_lines_for_model("usage", None);
        assert!(lines[0].contains("pricing=estimated-default"));
    }

    #[test]
    fn reconstructs_usage_from_session_messages() {
        let mut session = Session::new();
        session.messages = vec![ConversationMessage {
            role: MessageRole::Assistant,
            blocks: vec![ContentBlock::Text {
                text: "done".to_string(),
            }],
            usage: Some(TokenUsage {
                input_tokens: 5,
                output_tokens: 2,
                cache_creation_input_tokens: 1,
                cache_read_input_tokens: 0,
            }),
        }];

        let tracker = UsageTracker::from_session(&session);
        assert_eq!(tracker.turns(), 1);
        assert_eq!(tracker.cumulative_usage().total_tokens(), 8);
    }
}
