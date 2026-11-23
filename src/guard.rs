#[derive(Debug, Clone)]
pub struct GuardDecision {
    pub risk_score: f32,
    pub reasons: Vec<String>,
}

pub struct PromptGuard {
    patterns: Vec<(&'static str, f32, &'static str)>,
}

impl Default for PromptGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl PromptGuard {
    pub fn new() -> Self {
        Self {
            patterns: vec![
                ("ignore previous instructions", 0.55, "Instruction override attempt"),
                ("ignore all previous", 0.55, "Instruction override attempt"),
                ("disregard your system prompt", 0.65, "System prompt override attempt"),
                ("reveal system prompt", 0.60, "System prompt exfiltration attempt"),
                ("show me your system prompt", 0.60, "System prompt exfiltration attempt"),
                ("developer mode", 0.50, "Jailbreak framing detected"),
                ("jailbreak", 0.55, "Explicit jailbreak reference"),
                ("exfiltrate", 0.50, "Data exfiltration indicator"),
                ("dump environment variables", 0.60, "Environment secret exfiltration"),
                ("leak token", 0.50, "Credential leak attempt"),
            ],
        }
    }

    pub fn evaluate(&self, text: &str) -> GuardDecision {
        let lower = text.to_ascii_lowercase();
        let mut score: f32 = 0.0;
        let mut reasons = Vec::new();

        for (needle, weight, label) in &self.patterns {
            if lower.contains(needle) {
                score = (score + *weight).min(1.0);
                if !reasons.contains(&label.to_string()) {
                    reasons.push(label.to_string());
                }
            }
        }

        GuardDecision {
            risk_score: score,
            reasons,
        }
    }
}
