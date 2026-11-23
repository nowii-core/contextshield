#[derive(Debug, Clone)]
pub struct RedactionOutcome {
    pub redacted_text: String,
    pub findings: Vec<String>,
}

pub struct SecretRedactor;

impl Default for SecretRedactor {
    fn default() -> Self {
        Self::new()
    }
}

impl SecretRedactor {
    pub fn new() -> Self {
        Self
    }

    pub fn redact(&self, input: &str) -> RedactionOutcome {
        let mut out = String::with_capacity(input.len());
        let mut findings = Vec::new();
        let bytes = input.as_bytes();
        let mut i = 0;

        while i < bytes.len() {
            if let Some((consumed, label)) = match_secret_token(&input[i..]) {
                out.push_str(&format!("[REDACTED:{}]", label));
                if !findings.contains(&label.to_string()) {
                    findings.push(label.to_string());
                }
                i += consumed;
            } else if let Some(ch) = input[i..].chars().next() {
                out.push(ch);
                i += ch.len_utf8();
            } else {
                break;
            }
        }

        RedactionOutcome {
            redacted_text: out,
            findings,
        }
    }
}

fn match_secret_token(slice: &str) -> Option<(usize, &'static str)> {
    if slice.starts_with("-----BEGIN ") && slice.contains("PRIVATE KEY-----") {
        if let Some(end_idx) = slice.find("PRIVATE KEY-----") {
            return Some((end_idx + "PRIVATE KEY-----".len(), "PRIVATE_KEY"));
        }
    }

    if slice.starts_with("AKIA") {
        let len = token_run_len(slice, 4);
        if len >= 16 {
            return Some((len, "AWS_ACCESS_KEY"));
        }
    }

    for prefix in ["ghp_", "gho_", "ghs_", "github_pat_"] {
        if slice.starts_with(prefix) {
            let len = token_run_len(slice, prefix.len());
            if len > prefix.len() + 6 {
                return Some((len, "GITHUB_TOKEN"));
            }
        }
    }

    for prefix in ["sk-live-", "sk-test-", "sk-proj-", "sk-"] {
        if slice.starts_with(prefix) {
            let len = token_run_len(slice, prefix.len());
            if len > prefix.len() + 8 {
                return Some((len, "API_KEY"));
            }
        }
    }

    let lower = slice.to_ascii_lowercase();
    if lower.starts_with("bearer ") {
        let after_space = &slice[7..];
        let trimmed_ws = after_space.len() - after_space.trim_start_matches([' ', '\t']).len();
        let token_start = 7 + trimmed_ws;
        let token_len = token_run_len(&slice[token_start..], 0);
        if token_len >= 10 {
            return Some((token_start + token_len, "BEARER_TOKEN"));
        }
    }

    None
}

fn token_run_len(s: &str, start_offset: usize) -> usize {
    let mut total = start_offset;
    for ch in s[start_offset..].chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '+') {
            total += ch.len_utf8();
        } else {
            break;
        }
    }
    total
}
