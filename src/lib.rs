pub mod cache;
pub mod guard;
pub mod redactor;
pub mod telemetry;

use cache::{CacheLookup, SemanticCache};
use guard::{GuardDecision, PromptGuard};
use redactor::{RedactionOutcome, SecretRedactor};
use std::time::Instant;
use telemetry::{GatewayTelemetry, TelemetrySnapshot};

#[derive(Debug, Clone)]
pub struct GatewayConfig {
    pub similarity_threshold: f32,
    pub cache_capacity: usize,
    pub cache_ttl_secs: u64,
    pub block_threshold: f32,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            similarity_threshold: 0.78,
            cache_capacity: 1024,
            cache_ttl_secs: 3600,
            block_threshold: 0.65,
        }
    }
}

#[derive(Debug, Clone)]
pub struct InspectResult {
    pub original_prompt: String,
    pub sanitized_prompt: String,
    pub redactions: Vec<String>,
    pub risk_score: f32,
    pub risk_reasons: Vec<String>,
    pub blocked: bool,
}

#[derive(Debug, Clone)]
pub struct CompletionResult {
    pub blocked: bool,
    pub block_reason: Option<String>,
    pub cache_hit: bool,
    pub similarity: Option<f32>,
    pub sanitized_prompt: String,
    pub response: String,
    pub risk_score: f32,
    pub redactions: Vec<String>,
    pub latency_us: u128,
}

pub struct ContextShield {
    config: GatewayConfig,
    redactor: SecretRedactor,
    guard: PromptGuard,
    cache: SemanticCache,
    telemetry: GatewayTelemetry,
}

impl ContextShield {
    pub fn new(config: GatewayConfig) -> Self {
        let cache = SemanticCache::new(
            config.cache_capacity,
            config.similarity_threshold,
            config.cache_ttl_secs,
            128,
        );
        Self {
            config,
            redactor: SecretRedactor::new(),
            guard: PromptGuard::new(),
            cache,
            telemetry: GatewayTelemetry::new(),
        }
    }

    pub fn inspect(&self, prompt: &str) -> InspectResult {
        let RedactionOutcome {
            redacted_text,
            findings,
        } = self.redactor.redact(prompt);

        let GuardDecision {
            risk_score,
            reasons,
            ..
        } = self.guard.evaluate(&redacted_text);

        let blocked = risk_score >= self.config.block_threshold;
        InspectResult {
            original_prompt: prompt.to_string(),
            sanitized_prompt: redacted_text,
            redactions: findings,
            risk_score,
            risk_reasons: reasons,
            blocked,
        }
    }

    pub fn complete(&mut self, prompt: &str) -> CompletionResult {
        let start = Instant::now();

        let RedactionOutcome {
            redacted_text,
            findings,
        } = self.redactor.redact(prompt);

        if !findings.is_empty() {
            self.telemetry.record_redactions(findings.len() as u64);
        }

        let decision = self.guard.evaluate(&redacted_text);
        let blocked = decision.risk_score >= self.config.block_threshold;

        if blocked {
            let elapsed_us = start.elapsed().as_micros();
            self.telemetry.record_request(false, true, elapsed_us);
            let reason = if decision.reasons.is_empty() {
                "Prompt exceeded injection risk threshold".to_string()
            } else {
                decision.reasons.join("; ")
            };
            return CompletionResult {
                blocked: true,
                block_reason: Some(reason.clone()),
                cache_hit: false,
                similarity: None,
                sanitized_prompt: redacted_text,
                response: format!("[BLOCKED] {}", reason),
                risk_score: decision.risk_score,
                redactions: findings,
                latency_us: elapsed_us,
            };
        }

        match self.cache.lookup(&redacted_text) {
            CacheLookup::Hit {
                response,
                similarity,
            } => {
                let elapsed_us = start.elapsed().as_micros();
                self.telemetry.record_request(true, false, elapsed_us);
                CompletionResult {
                    blocked: false,
                    block_reason: None,
                    cache_hit: true,
                    similarity: Some(similarity),
                    sanitized_prompt: redacted_text,
                    response,
                    risk_score: decision.risk_score,
                    redactions: findings,
                    latency_us: elapsed_us,
                }
            }
            CacheLookup::Miss => {
                let synthesized = synthesize_local_response(&redacted_text);
                self.cache.insert(&redacted_text, &synthesized);
                let elapsed_us = start.elapsed().as_micros();
                self.telemetry.record_request(false, false, elapsed_us);
                CompletionResult {
                    blocked: false,
                    block_reason: None,
                    cache_hit: false,
                    similarity: None,
                    sanitized_prompt: redacted_text,
                    response: synthesized,
                    risk_score: decision.risk_score,
                    redactions: findings,
                    latency_us: elapsed_us,
                }
            }
        }
    }

    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    pub fn stats(&self) -> TelemetrySnapshot {
        self.telemetry.snapshot(self.cache.len())
    }
}

fn synthesize_local_response(prompt: &str) -> String {
    let trimmed = prompt.trim();
    if trimmed.is_empty() {
        return "Empty prompt received.".to_string();
    }
    let word_count = trimmed.split_whitespace().count();
    format!(
        "ContextShield processed {} tokens safely: '{}'",
        word_count, trimmed
    )
}

pub fn json_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 8);
    for ch in input.chars() {
        match ch {
            '"' => out.push_str("\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

pub fn extract_json_string_field(body: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\"", key);
    let key_pos = body.find(&needle)?;
    let after_key = &body[key_pos + needle.len()..];
    let colon_pos = after_key.find(':')?;
    let after_colon = after_key[colon_pos + 1..].trim_start();
    if !after_colon.starts_with('"') {
        return None;
    }
    let chars = &after_colon[1..];
    let mut out = String::new();
    let mut escaped = false;
    for ch in chars.chars() {
        if escaped {
            match ch {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                other => out.push(other),
            }
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return Some(out);
        } else {
            out.push(ch);
        }
    }
    None
}
