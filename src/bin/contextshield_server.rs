use contextshield::{
    extract_json_string_field, json_escape, ContextShield, GatewayConfig,
};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

fn main() {
    let addr = "127.0.0.1:8081";
    let listener = TcpListener::bind(addr).expect("failed to bind ContextShield HTTP server");
    let shield = Arc::new(Mutex::new(ContextShield::new(GatewayConfig::default())));

    println!("ContextShield HTTP gateway listening on http://{}", addr);

    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                handle_connection(&mut stream, &shield);
            }
            Err(err) => {
                eprintln!("connection error: {}", err);
            }
        }
    }
}

fn handle_connection(stream: &mut TcpStream, shield: &Arc<Mutex<ContextShield>>) {
    let mut buf = [0_u8; 65536];
    let n = match stream.read(&mut buf) {
        Ok(0) | Err(_) => return,
        Ok(n) => n,
    };

    let raw = String::from_utf8_lossy(&buf[..n]);
    let mut lines = raw.lines();
    let request_line = match lines.next() {
        Some(line) => line,
        None => return,
    };

    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("/");

    let body = if let Some(idx) = raw.find("\r\n\r\n") {
        &raw[idx + 4..]
    } else if let Some(idx) = raw.find("\n\n") {
        &raw[idx + 2..]
    } else {
        ""
    };

    let (status, content_type, payload) = match (method, path) {
        ("GET", "/health") => (
            "200 OK",
            "application/json",
            "{\"status\":\"ok\",\"service\":\"contextshield\",\"version\":\"0.1.0\"}".to_string(),
        ),
        ("GET", "/stats") => {
            let guard = shield.lock().unwrap();
            let snap = guard.stats();
            let json = format!(
                "{{\"total_requests\":{},\"cache_hits\":{},\"cache_misses\":{},\"blocked_requests\":{},\"redacted_secrets\":{},\"cache_entries\":{},\"p50_us\":{},\"p95_us\":{},\"p99_us\":{}}}",
                snap.total_requests,
                snap.cache_hits,
                snap.cache_misses,
                snap.blocked_requests,
                snap.redacted_secrets,
                snap.cache_entries,
                snap.p50_us,
                snap.p95_us,
                snap.p99_us
            );
            ("200 OK", "application/json", json)
        }
        ("POST", "/cache/clear") => {
            let mut guard = shield.lock().unwrap();
            guard.clear_cache();
            (
                "200 OK",
                "application/json",
                "{\"cleared\":true}".to_string(),
            )
        }
        ("POST", "/inspect") => {
            let prompt = extract_json_string_field(body, "prompt").unwrap_or_default();
            let guard = shield.lock().unwrap();
            let res = guard.inspect(&prompt);
            let reasons_json: Vec<String> = res
                .risk_reasons
                .iter()
                .map(|r| format!("\"{}\"", json_escape(r)))
                .collect();
            let redactions_json: Vec<String> = res
                .redactions
                .iter()
                .map(|r| format!("\"{}\"", json_escape(r)))
                .collect();
            let json = format!(
                "{{\"sanitized_prompt\":\"{}\",\"redactions\":[{}],\"risk_score\":{:.2},\"risk_reasons\":[{}],\"blocked\":{}}}",
                json_escape(&res.sanitized_prompt),
                redactions_json.join(","),
                res.risk_score,
                reasons_json.join(","),
                res.blocked
            );
            ("200 OK", "application/json", json)
        }
        ("POST", "/complete") => {
            let prompt = extract_json_string_field(body, "prompt").unwrap_or_default();
            let mut guard = shield.lock().unwrap();
            let res = guard.complete(&prompt);
            let status_code = if res.blocked { "403 Forbidden" } else { "200 OK" };
            let sim_str = match res.similarity {
                Some(s) => format!("{:.4}", s),
                None => "null".to_string(),
            };
            let block_reason_str = match &res.block_reason {
                Some(r) => format!("\"{}\"", json_escape(r)),
                None => "null".to_string(),
            };
            let json = format!(
                "{{\"blocked\":{},\"block_reason\":{},\"cache_hit\":{},\"similarity\":{},\"sanitized_prompt\":\"{}\",\"response\":\"{}\",\"risk_score\":{:.2},\"latency_us\":{}}}",
                res.blocked,
                block_reason_str,
                res.cache_hit,
                sim_str,
                json_escape(&res.sanitized_prompt),
                json_escape(&res.response),
                res.risk_score,
                res.latency_us
            );
            (status_code, "application/json", json)
        }
        _ => (
            "404 Not Found",
            "application/json",
            "{\"error\":\"not_found\"}".to_string(),
        ),
    };

    let response = format!(
        "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        status,
        content_type,
        payload.len(),
        payload
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}
