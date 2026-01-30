// Text Chunking Module
// Splits text into chunks with configurable size and overlap

use crate::memory::types::{MemoryError, MemoryResult, MIN_CHUNK_LENGTH};
use tiktoken_rs::cl100k_base;

/// A text chunk with metadata
#[derive(Debug, Clone)]
pub struct TextChunk {
    pub content: String,
    pub token_count: usize,
    pub start_index: usize,
    pub end_index: usize,
}

/// Chunking configuration
#[derive(Debug, Clone)]
pub struct ChunkingConfig {
    /// Target chunk size in tokens
    pub chunk_size: usize,
    /// Overlap between chunks in tokens
    pub chunk_overlap: usize,
    /// Separator to use for splitting (if None, uses token boundaries)
    pub separator: Option<String>,
}

impl Default for ChunkingConfig {
    fn default() -> Self {
        Self {
            chunk_size: 512,
            chunk_overlap: 64,
            separator: None,
        }
    }
}

/// Tokenizer wrapper for counting tokens
pub struct Tokenizer {
    bpe: tiktoken_rs::CoreBPE,
}

impl Tokenizer {
    pub fn new() -> MemoryResult<Self> {
        let bpe = cl100k_base().map_err(|e| MemoryError::Tokenization(e.to_string()))?;
        Ok(Self { bpe })
    }

    /// Count tokens in text
    pub fn count_tokens(&self, text: &str) -> usize {
        self.bpe.encode_with_special_tokens(text).len()
    }

    /// Encode text to tokens
    pub fn encode(&self, text: &str) -> Vec<u32> {
        self.bpe.encode_with_special_tokens(text)
    }

    /// Decode tokens to text
    pub fn decode(&self, tokens: &[u32]) -> String {
        self.bpe.decode(tokens.to_vec()).unwrap_or_default()
    }
}

impl Default for Tokenizer {
    fn default() -> Self {
        Self::new().expect("Failed to initialize tokenizer")
    }
}

/// Chunk text into pieces with overlap
pub fn chunk_text(text: &str, config: &ChunkingConfig) -> MemoryResult<Vec<TextChunk>> {
    if text.is_empty() {
        return Ok(Vec::new());
    }

    // If text is short enough, return as single chunk
    if text.len() < MIN_CHUNK_LENGTH {
        let tokenizer = Tokenizer::new()?;
        let token_count = tokenizer.count_tokens(text);
        return Ok(vec![TextChunk {
            content: text.to_string(),
            token_count,
            start_index: 0,
            end_index: text.len(),
        }]);
    }

    let tokenizer = Tokenizer::new()?;
    let tokens = tokenizer.encode(text);

    if tokens.len() <= config.chunk_size {
        // Text fits in a single chunk
        return Ok(vec![TextChunk {
            content: text.to_string(),
            token_count: tokens.len(),
            start_index: 0,
            end_index: text.len(),
        }]);
    }

    let mut chunks = Vec::new();
    let mut start = 0;

    while start < tokens.len() {
        let end = (start + config.chunk_size).min(tokens.len());
        let chunk_tokens = &tokens[start..end];
        let chunk_text = tokenizer.decode(chunk_tokens);

        // Find character boundaries
        let start_char = if start == 0 {
            0
        } else {
            // Find the character position that corresponds to this token
            let prev_tokens = &tokens[..start];
            tokenizer.decode(prev_tokens).len()
        };

        let end_char = start_char + chunk_text.len();

        chunks.push(TextChunk {
            content: chunk_text,
            token_count: chunk_tokens.len(),
            start_index: start_char,
            end_index: end_char.min(text.len()),
        });

        // Move start forward by chunk_size - overlap
        let step = config.chunk_size.saturating_sub(config.chunk_overlap);
        if step == 0 {
            // Prevent infinite loop if overlap equals or exceeds chunk_size
            start = end;
        } else {
            start += step;
        }

        // Ensure we make progress
        if start >= tokens.len() {
            break;
        }
    }

    Ok(chunks)
}

/// Chunk text using semantic boundaries (paragraphs, sentences)
pub fn chunk_text_semantic(text: &str, config: &ChunkingConfig) -> MemoryResult<Vec<TextChunk>> {
    if text.is_empty() {
        return Ok(Vec::new());
    }

    // If text is short enough, return as single chunk
    if text.len() < MIN_CHUNK_LENGTH {
        let tokenizer = Tokenizer::new()?;
        let token_count = tokenizer.count_tokens(text);
        return Ok(vec![TextChunk {
            content: text.to_string(),
            token_count,
            start_index: 0,
            end_index: text.len(),
        }]);
    }

    let tokenizer = Tokenizer::new()?;
    let tokens = tokenizer.encode(text);

    if tokens.len() <= config.chunk_size {
        return Ok(vec![TextChunk {
            content: text.to_string(),
            token_count: tokens.len(),
            start_index: 0,
            end_index: text.len(),
        }]);
    }

    // Split by paragraphs first
    let paragraphs: Vec<&str> = text
        .split("\n\n")
        .filter(|p| !p.trim().is_empty())
        .collect();

    let mut chunks = Vec::new();
    let mut current_chunk = String::new();
    let mut current_tokens = 0;
    let mut chunk_start = 0;
    let mut current_pos = 0;

    for paragraph in paragraphs {
        let para_tokens = tokenizer.count_tokens(paragraph);

        if para_tokens > config.chunk_size {
            // Paragraph is too long, split by sentences
            if !current_chunk.is_empty() {
                chunks.push(TextChunk {
                    content: current_chunk.clone(),
                    token_count: current_tokens,
                    start_index: chunk_start,
                    end_index: current_pos,
                });
                current_chunk.clear();
                current_tokens = 0;
                chunk_start = current_pos;
            }

            // Split long paragraph by sentences
            let sentences: Vec<&str> = paragraph
                .split(|c| c == '.' || c == '!' || c == '?')
                .filter(|s| !s.trim().is_empty())
                .collect();

            for sentence in sentences {
                let sentence_with_punct = format!("{}.", sentence.trim());
                let sent_tokens = tokenizer.count_tokens(&sentence_with_punct);

                if current_tokens + sent_tokens > config.chunk_size && !current_chunk.is_empty() {
                    chunks.push(TextChunk {
                        content: current_chunk.clone(),
                        token_count: current_tokens,
                        start_index: chunk_start,
                        end_index: current_pos,
                    });
                    // Keep overlap
                    let overlap_tokens =
                        current_tokens.saturating_sub(config.chunk_size - config.chunk_overlap);
                    if overlap_tokens > 0 && overlap_tokens < current_tokens {
                        let overlap_text =
                            get_last_n_tokens(&tokenizer, &current_chunk, overlap_tokens);
                        current_chunk = overlap_text;
                        current_tokens = overlap_tokens;
                        chunk_start = current_pos - current_chunk.len();
                    } else {
                        current_chunk.clear();
                        current_tokens = 0;
                        chunk_start = current_pos;
                    }
                }

                current_chunk.push_str(&sentence_with_punct);
                current_chunk.push(' ');
                current_tokens += sent_tokens;
                current_pos += sentence_with_punct.len() + 1;
            }
        } else if current_tokens + para_tokens > config.chunk_size {
            // Start new chunk
            if !current_chunk.is_empty() {
                chunks.push(TextChunk {
                    content: current_chunk.clone(),
                    token_count: current_tokens,
                    start_index: chunk_start,
                    end_index: current_pos,
                });
            }
            current_chunk = paragraph.to_string();
            current_chunk.push('\n');
            current_tokens = para_tokens;
            chunk_start = current_pos;
            current_pos += paragraph.len() + 1;
        } else {
            // Add to current chunk
            current_chunk.push_str(paragraph);
            current_chunk.push('\n');
            current_tokens += para_tokens;
            current_pos += paragraph.len() + 1;
        }
    }

    // Don't forget the last chunk
    if !current_chunk.is_empty() {
        chunks.push(TextChunk {
            content: current_chunk.trim().to_string(),
            token_count: current_tokens,
            start_index: chunk_start,
            end_index: text.len(),
        });
    }

    Ok(chunks)
}

/// Get the last n tokens from text
fn get_last_n_tokens(tokenizer: &Tokenizer, text: &str, n: usize) -> String {
    let tokens = tokenizer.encode(text);
    let start = tokens.len().saturating_sub(n);
    let last_tokens = &tokens[start..];
    tokenizer.decode(last_tokens)
}

/// Estimate token count without full tokenization (faster but less accurate)
pub fn estimate_token_count(text: &str) -> usize {
    // Rough estimate: ~4 characters per token on average for English
    text.len() / 4
}

/// Truncate text to fit within token budget
pub fn truncate_to_tokens(text: &str, max_tokens: usize) -> MemoryResult<String> {
    let tokenizer = Tokenizer::new()?;
    let tokens = tokenizer.encode(text);

    if tokens.len() <= max_tokens {
        Ok(text.to_string())
    } else {
        let truncated = &tokens[..max_tokens];
        Ok(tokenizer.decode(truncated))
    }
}

/// Merge small chunks to reduce overhead
pub fn merge_small_chunks(chunks: Vec<TextChunk>, min_tokens: usize) -> Vec<TextChunk> {
    if chunks.len() < 2 {
        return chunks;
    }

    let mut merged = Vec::new();
    let mut current = chunks[0].clone();

    for chunk in chunks.into_iter().skip(1) {
        if current.token_count < min_tokens {
            // Merge with current
            current.content.push('\n');
            current.content.push_str(&chunk.content);
            current.token_count += chunk.token_count;
            current.end_index = chunk.end_index;
        } else {
            merged.push(current);
            current = chunk;
        }
    }

    merged.push(current);
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_text_empty() {
        let config = ChunkingConfig::default();
        let chunks = chunk_text("", &config).unwrap();
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_chunk_text_short() {
        let config = ChunkingConfig::default();
        let text = "This is a short text.";
        let chunks = chunk_text(text, &config).unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].content, text);
    }

    #[test]
    fn test_chunk_text_long() {
        let config = ChunkingConfig {
            chunk_size: 10,
            chunk_overlap: 2,
            separator: None,
        };
        let text = "This is a much longer text that needs to be split into multiple chunks. It contains several sentences and should be broken up appropriately.";
        let chunks = chunk_text(text, &config).unwrap();
        assert!(chunks.len() > 1);

        // Check overlap
        for i in 1..chunks.len() {
            let prev_end = chunks[i - 1].end_index;
            let curr_start = chunks[i].start_index;
            assert!(curr_start < prev_end, "Chunks should overlap");
        }
    }

    #[test]
    fn test_tokenizer_count() {
        let tokenizer = Tokenizer::new().unwrap();
        let count = tokenizer.count_tokens("Hello world");
        assert!(count > 0);
    }

    #[test]
    fn test_estimate_token_count() {
        let text = "This is a test sentence with approximately twelve tokens.";
        let estimated = estimate_token_count(text);
        let tokenizer = Tokenizer::new().unwrap();
        let actual = tokenizer.count_tokens(text);

        // Estimate should be in the ballpark
        let diff = (estimated as i64 - actual as i64).abs();
        assert!(diff < 5, "Estimate should be close to actual");
    }
}
