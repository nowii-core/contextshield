use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub enum CacheLookup {
    Hit { response: String, similarity: f32 },
    Miss,
}

#[derive(Debug, Clone)]
struct CacheEntry {
    embedding: Vec<f32>,
    response: String,
    inserted_at: Instant,
}

pub struct SemanticCache {
    entries: Vec<CacheEntry>,
    capacity: usize,
    similarity_threshold: f32,
    ttl: Duration,
    dims: usize,
}

impl SemanticCache {
    pub fn new(capacity: usize, similarity_threshold: f32, ttl_secs: u64, dims: usize) -> Self {
        Self {
            entries: Vec::with_capacity(capacity.min(256)),
            capacity: capacity.max(1),
            similarity_threshold: similarity_threshold.clamp(0.0, 1.0),
            ttl: Duration::from_secs(ttl_secs),
            dims: dims.max(16),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn lookup(&mut self, text: &str) -> CacheLookup {
        self.evict_expired();
        let query_vec = embed_text(text, self.dims);

        let mut best_idx: Option<usize> = None;
        let mut best_sim: f32 = -1.0;

        for (idx, entry) in self.entries.iter().enumerate() {
            let sim = cosine_similarity(&query_vec, &entry.embedding);
            if sim >= self.similarity_threshold && sim > best_sim {
                best_sim = sim;
                best_idx = Some(idx);
            }
        }

        if let Some(idx) = best_idx {
            let entry = self.entries.remove(idx);
            let response = entry.response.clone();
            self.entries.push(CacheEntry {
                inserted_at: Instant::now(),
                ..entry
            });
            return CacheLookup::Hit {
                response,
                similarity: best_sim,
            };
        }

        CacheLookup::Miss
    }

    pub fn insert(&mut self, text: &str, response: &str) {
        self.evict_expired();
        if self.entries.len() >= self.capacity {
            self.entries.remove(0);
        }
        let embedding = embed_text(text, self.dims);
        self.entries.push(CacheEntry {
            embedding,
            response: response.to_string(),
            inserted_at: Instant::now(),
        });
    }

    fn evict_expired(&mut self) {
        let ttl = self.ttl;
        self.entries
            .retain(|entry| entry.inserted_at.elapsed() <= ttl);
    }
}

pub fn embed_text(text: &str, dims: usize) -> Vec<f32> {
    let mut vec = vec![0.0_f32; dims];
    let normalized = text.to_ascii_lowercase();
    let tokens: Vec<String> = normalized
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() >= 2)
        .map(normalize_token)
        .collect();

    if tokens.is_empty() {
        return vec;
    }

    for token in &tokens {
        let idx = (fnv1a_64(token.as_bytes()) as usize) % dims;
        let sign = if (fnv1a_64(format!("s:{}", token).as_bytes()) & 1) == 0 {
            1.0
        } else {
            -1.0
        };
        vec[idx] += sign;
    }

    for pair in tokens.windows(2) {
        let bigram = format!("{}_{}", pair[0], pair[1]);
        let idx = (fnv1a_64(bigram.as_bytes()) as usize) % dims;
        vec[idx] += 0.5;
    }

    let norm = vec.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm > 0.0 {
        for v in &mut vec {
            *v /= norm;
        }
    }
    vec
}

fn normalize_token(tok: &str) -> String {
    if tok.len() > 4 && tok.ends_with('s') && !tok.ends_with("ss") {
        tok[..tok.len() - 1].to_string()
    } else {
        tok.to_string()
    }
}

fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}
