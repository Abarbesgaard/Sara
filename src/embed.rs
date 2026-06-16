use anyhow::{Context, Result};
use reqwest::blocking::Client;
use rusqlite::Connection;
use uuid::Uuid;

use crate::config::{Config, LlmConfig};

pub fn embed_text(cfg: &Config, text: &str) -> Result<Vec<f32>> {
    match cfg.effective_embeddings_provider().as_str() {
        "azure" | "azure_openai" => embed_azure(cfg, text),
        "openai" => embed_openai(cfg, text),
        _ => embed_ollama(cfg, text),
    }
}

fn embed_ollama(cfg: &Config, text: &str) -> Result<Vec<f32>> {
    let emb = &cfg.embeddings;
    let base = emb
        .base_url
        .as_deref()
        .unwrap_or("http://localhost:11434");
    let url = format!("{base}/api/embeddings");
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()?;
    let body = serde_json::json!({
        "model": cfg.embeddings.model,
        "prompt": text,
    });
    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .context("Embeddings request failed (is Ollama running?)")?;
    let json: serde_json::Value = resp.json()?;
    if let Some(err) = json.get("error").and_then(|v| v.as_str()) {
        anyhow::bail!("{err} (run `ollama pull {}`)", cfg.embeddings.model);
    }
    let embedding = json
        .get("embedding")
        .and_then(|v| v.as_array())
        .context("No embedding in response")?;
    Ok(parse_embedding_array(embedding))
}

fn embed_openai(cfg: &Config, text: &str) -> Result<Vec<f32>> {
    let llm = cfg.effective_llm();
    let base = llm
        .base_url
        .as_deref()
        .unwrap_or("https://api.openai.com");
    let url = format!("{base}/v1/embeddings");
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()?;
    let body = serde_json::json!({
        "model": cfg.effective_embeddings_model(),
        "input": text,
    });
    let resp = client
        .post(&url)
        .bearer_auth(llm.api_key.as_deref().unwrap_or(""))
        .json(&body)
        .send()
        .context("OpenAI embeddings request failed")?;
    let json: serde_json::Value = resp.error_for_status()?.json()?;
    extract_openai_embedding(&json)
}

fn embed_azure(cfg: &Config, text: &str) -> Result<Vec<f32>> {
    let llm = cfg.effective_llm();
    let deployment = cfg.effective_embeddings_model();
    let (base_url, _) = azure_base_and_deployment(llm, &deployment);
    let url = format!(
        "{base_url}/openai/deployments/{deployment}/embeddings?api-version=2024-08-01-preview"
    );
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()?;
    let body = serde_json::json!({ "input": text });
    let resp = client
        .post(&url)
        .header("api-key", llm.api_key.as_deref().unwrap_or(""))
        .json(&body)
        .send()
        .context("Azure embeddings request failed")?;
    let json: serde_json::Value = resp.error_for_status()?.json()?;
    extract_openai_embedding(&json)
}

fn azure_base_and_deployment(llm: &LlmConfig, embedding_model: &str) -> (String, String) {
    let resource = llm
        .base_url
        .clone()
        .unwrap_or_else(|| "https://my-resource.openai.azure.com".to_string());
    let base_url = if resource.starts_with("http") {
        resource
    } else {
        format!("https://{resource}.openai.azure.com")
    };
    (base_url, embedding_model.to_string())
}

fn extract_openai_embedding(json: &serde_json::Value) -> Result<Vec<f32>> {
    let embedding = json["data"][0]["embedding"]
        .as_array()
        .context("No embedding in response")?;
    Ok(parse_embedding_array(embedding))
}

fn parse_embedding_array(embedding: &[serde_json::Value]) -> Vec<f32> {
    embedding
        .iter()
        .filter_map(|v| v.as_f64().map(|f| f as f32))
        .collect()
}

pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na * nb)
    }
}

pub fn embed_and_store(conn: &Connection, cfg: &Config, ref_uuid: &Uuid, text: &str) -> Result<()> {
    if text.trim().is_empty() {
        return Ok(());
    }
    match embed_text(cfg, text) {
        Ok(vec) => {
            crate::db::upsert_embedding(conn, ref_uuid, &vec)?;
            Ok(())
        }
        Err(e) => {
            eprintln!("Warning: could not embed: {e}");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_identical_vectors() {
        let v = vec![1.0, 0.0, 0.0];
        assert!((cosine_similarity(&v, &v) - 1.0).abs() < 0.001);
    }
}
