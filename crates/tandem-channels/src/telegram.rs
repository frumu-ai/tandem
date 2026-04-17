//! Telegram channel adapter for Tandem.
//!
//! Uses the Bot API long-polling (`getUpdates` with `timeout=25`) to receive
//! messages and `sendMessage` to deliver responses. Messages are split into
//! 4096-character chunks to comply with Telegram's limit.

use std::sync::Arc;
use std::time::Duration;
use std::{path::PathBuf, time::SystemTime};

use async_trait::async_trait;
use parking_lot::Mutex;
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use reqwest::Client;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{debug, error, warn};

use crate::config::{is_user_allowed, TelegramConfig, TelegramStyleProfile};
use crate::traits::{
    should_accept_message, Channel, ChannelMessage, ConversationScope, ConversationScopeKind,
    MessageTriggerContext, SendMessage, TriggerSource,
};

const MAX_MESSAGE_LEN: usize = 4096;
const TELEGRAM_API: &str = "https://api.telegram.org/bot";

/// Split a long message into ≤4096-character chunks.
pub fn split_message(text: &str) -> Vec<String> {
    if text.chars().count() <= MAX_MESSAGE_LEN {
        return vec![text.to_string()];
    }

    #[derive(Default, Clone, Copy)]
    struct MarkdownV2State {
        escape_next: bool,
        in_code_block: bool,
        in_inline_code: bool,
        in_link_text: bool,
        in_link_dest: bool,
        await_link_dest: bool,
        in_bold: bool,
        in_italic: bool,
        in_strike: bool,
        in_underline: bool,
    }

    impl MarkdownV2State {
        fn is_neutral(self) -> bool {
            !self.escape_next
                && !self.in_code_block
                && !self.in_inline_code
                && !self.in_link_text
                && !self.in_link_dest
                && !self.await_link_dest
                && !self.in_bold
                && !self.in_italic
                && !self.in_strike
                && !self.in_underline
        }
    }

    fn byte_index_at_char_limit(text: &str, max_chars: usize) -> usize {
        text.char_indices()
            .nth(max_chars)
            .map_or(text.len(), |(idx, _)| idx)
    }

    let mut chunks = Vec::new();
    let mut start = 0usize;

    while start < text.len() {
        let remaining = &text[start..];
        if remaining.chars().count() <= MAX_MESSAGE_LEN {
            chunks.push(remaining.to_string());
            break;
        }

        let mut state = MarkdownV2State::default();
        let mut offset = 0usize;
        let mut chars_used = 0usize;
        let mut last_safe_any: Option<usize> = None;
        let mut last_safe_space: Option<usize> = None;
        let mut last_safe_newline: Option<usize> = None;

        while offset < remaining.len() && chars_used < MAX_MESSAGE_LEN {
            let rest = &remaining[offset..];
            let mut consumed_bytes = 0usize;
            let mut consumed_chars = 0usize;
            let mut last_char = '\0';

            if state.escape_next {
                let ch = rest.chars().next().unwrap_or('\0');
                state.escape_next = false;
                consumed_bytes = ch.len_utf8();
                consumed_chars = 1;
                last_char = ch;
            } else if !state.in_inline_code && rest.starts_with("```") {
                state.in_code_block = !state.in_code_block;
                consumed_bytes = 3;
                consumed_chars = 3;
                last_char = '`';
            } else {
                let ch = rest.chars().next().unwrap_or('\0');
                if !state.in_code_block && !state.in_inline_code && ch == '\\' {
                    state.escape_next = true;
                } else if !state.in_code_block && ch == '`' {
                    state.in_inline_code = !state.in_inline_code;
                } else if !state.in_code_block && !state.in_inline_code {
                    if state.in_link_dest {
                        if ch == ')' {
                            state.in_link_dest = false;
                        }
                    } else if state.in_link_text {
                        if ch == ']' {
                            state.in_link_text = false;
                            state.await_link_dest = true;
                        }
                    } else if state.await_link_dest {
                        if ch == '(' {
                            state.in_link_dest = true;
                            state.await_link_dest = false;
                        } else if !ch.is_whitespace() {
                            state.await_link_dest = false;
                        }
                    } else if ch == '[' {
                        state.in_link_text = true;
                    } else if rest.starts_with("__") {
                        state.in_underline = !state.in_underline;
                        consumed_bytes = 2;
                        consumed_chars = 2;
                        last_char = '_';
                    } else if ch == '*' {
                        state.in_bold = !state.in_bold;
                    } else if ch == '_' {
                        state.in_italic = !state.in_italic;
                    } else if ch == '~' {
                        state.in_strike = !state.in_strike;
                    }
                }

                if consumed_bytes == 0 {
                    consumed_bytes = ch.len_utf8();
                    consumed_chars = 1;
                    last_char = ch;
                }
            }

            if chars_used + consumed_chars > MAX_MESSAGE_LEN {
                break;
            }

            offset += consumed_bytes;
            chars_used += consumed_chars;
            let boundary = start + offset;

            if state.is_neutral() {
                last_safe_any = Some(boundary);
                if last_char == '\n' {
                    last_safe_newline = Some(boundary);
                } else if last_char.is_whitespace() {
                    last_safe_space = Some(boundary);
                }
            }
        }

        let mut split_at = last_safe_newline
            .or(last_safe_space)
            .or(last_safe_any)
            .unwrap_or(start + byte_index_at_char_limit(remaining, MAX_MESSAGE_LEN));
        if split_at <= start {
            split_at = start + byte_index_at_char_limit(remaining, MAX_MESSAGE_LEN);
        }

        chunks.push(text[start..split_at].to_string());
        start = split_at;
    }

    chunks
}

fn apply_telegram_style_profile(input: &str, profile: TelegramStyleProfile) -> String {
    match profile {
        TelegramStyleProfile::Default => input.to_string(),
        TelegramStyleProfile::Compact => compact_telegram_text(input),
        TelegramStyleProfile::Friendly => transform_profile_lines(input, "✨", "•"),
        TelegramStyleProfile::Ops => transform_profile_lines(input, "▶", "▪"),
    }
}

fn compact_telegram_text(input: &str) -> String {
    let mut out = String::new();
    let mut blank_run = 0usize;
    for line in input.lines() {
        let trimmed_end = line.trim_end();
        if trimmed_end.trim().is_empty() {
            blank_run += 1;
            if blank_run > 1 {
                continue;
            }
            if !out.is_empty() {
                out.push('\n');
            }
            continue;
        }
        blank_run = 0;
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(trimmed_end);
    }
    if out.is_empty() {
        input.trim().to_string()
    } else {
        out
    }
}

fn transform_profile_lines(input: &str, heading_prefix: &str, bullet_prefix: &str) -> String {
    let compacted = compact_telegram_text(input);
    let mut out = Vec::new();
    let mut in_fence = false;
    for line in compacted.lines() {
        let trimmed_start = line.trim_start();
        if trimmed_start.starts_with("```") {
            in_fence = !in_fence;
            out.push(line.to_string());
            continue;
        }
        if in_fence {
            out.push(line.to_string());
            continue;
        }

        let indent_len = line.len().saturating_sub(trimmed_start.len());
        let indent = &line[..indent_len];
        let transformed = if let Some(rest) = parse_markdown_heading(trimmed_start) {
            format!("{indent}{heading_prefix} {rest}")
        } else if let Some(rest) = parse_markdown_bullet(trimmed_start) {
            format!("{indent}{bullet_prefix} {rest}")
        } else {
            line.to_string()
        };
        out.push(transformed);
    }
    out.join("\n")
}

fn parse_markdown_heading(line: &str) -> Option<&str> {
    let bytes = line.as_bytes();
    let mut idx = 0usize;
    while idx < bytes.len() && bytes[idx] == b'#' && idx < 6 {
        idx += 1;
    }
    if idx == 0 || idx >= bytes.len() || bytes[idx] != b' ' {
        return None;
    }
    let heading = line[idx + 1..].trim();
    if heading.is_empty() {
        None
    } else {
        Some(heading)
    }
}

fn parse_markdown_bullet(line: &str) -> Option<&str> {
    if let Some(rest) = line
        .strip_prefix("- ")
        .or_else(|| line.strip_prefix("* "))
        .or_else(|| line.strip_prefix("+ "))
    {
        let cleaned = rest.trim();
        return if cleaned.is_empty() {
            None
        } else {
            Some(cleaned)
        };
    }
    let mut i = 0usize;
    let bytes = line.as_bytes();
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 || i + 1 >= bytes.len() || bytes[i] != b'.' || bytes[i + 1] != b' ' {
        return None;
    }
    let cleaned = line[i + 2..].trim();
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

pub fn format_markdown_for_telegram(input: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);
    let parser = Parser::new_ext(input, options);
    let mut writer = TelegramMarkdownV2Writer::default();
    for event in parser {
        writer.handle(event);
    }
    writer.finish()
}

#[derive(Default)]
struct TelegramMarkdownV2Writer {
    lines: Vec<String>,
    current: String,
    list_stack: Vec<ListState>,
    blockquote_depth: usize,
    in_code_block: bool,
    in_heading: bool,
    pending_link: Option<PendingLink>,
}

#[derive(Clone, Copy)]
enum ListState {
    Bullet,
    Ordered(u64),
}

struct PendingLink {
    text: String,
    dest: String,
}

impl TelegramMarkdownV2Writer {
    fn handle(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => self.push_text(&text),
            Event::Code(code) => self.push_inline_code(&code),
            Event::SoftBreak | Event::HardBreak => self.newline(),
            Event::Rule => {
                self.newline();
                self.push_text("---");
                self.newline();
            }
            Event::Html(html) | Event::InlineHtml(html) => self.push_text(&html),
            Event::TaskListMarker(checked) => {
                self.push_text(if checked { "[x] " } else { "[ ] " });
            }
            Event::FootnoteReference(_) => {}
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph if !self.current.is_empty() => {
                self.newline();
            }
            Tag::Heading { level, .. } => {
                let _ = level;
                if !self.current.is_empty() {
                    self.newline();
                }
                self.prefix();
                self.current.push('*');
                self.in_heading = true;
            }
            Tag::BlockQuote => {
                self.blockquote_depth += 1;
            }
            Tag::CodeBlock(kind) => {
                self.in_code_block = true;
                self.newline();
                match kind {
                    CodeBlockKind::Fenced(lang) => {
                        let lang = lang.trim();
                        if lang.is_empty() {
                            self.current.push_str("```");
                        } else {
                            self.current.push_str("```");
                            self.current.push_str(&sanitize_code_fence_lang(lang));
                        }
                    }
                    CodeBlockKind::Indented => self.current.push_str("```"),
                }
                self.newline();
            }
            Tag::List(start) => match start {
                Some(v) => self.list_stack.push(ListState::Ordered(v)),
                None => self.list_stack.push(ListState::Bullet),
            },
            Tag::Item => {
                if !self.current.is_empty() {
                    self.newline();
                }
                self.prefix();
                if let Some(last) = self.list_stack.last_mut() {
                    match last {
                        ListState::Bullet => self.current.push_str("\\- "),
                        ListState::Ordered(n) => {
                            self.current.push_str(&format!("{n}\\. "));
                            *n += 1;
                        }
                    }
                }
            }
            Tag::Emphasis if !self.in_code_block && self.pending_link.is_none() => {
                self.current.push('_');
            }
            Tag::Strong if !self.in_code_block && self.pending_link.is_none() => {
                self.current.push('*');
            }
            Tag::Strikethrough if !self.in_code_block && self.pending_link.is_none() => {
                self.current.push('~');
            }
            Tag::Link { dest_url, .. } => {
                self.pending_link = Some(PendingLink {
                    text: String::new(),
                    dest: dest_url.to_string(),
                });
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph | TagEnd::Item => self.newline(),
            TagEnd::Heading(_) => {
                if self.in_heading {
                    self.current.push('*');
                    self.in_heading = false;
                }
                self.newline();
            }
            TagEnd::BlockQuote => {
                self.blockquote_depth = self.blockquote_depth.saturating_sub(1);
                self.newline();
            }
            TagEnd::CodeBlock => {
                self.newline();
                self.current.push_str("```");
                self.in_code_block = false;
                self.newline();
            }
            TagEnd::List(_) => {
                self.list_stack.pop();
                self.newline();
            }
            TagEnd::Emphasis if !self.in_code_block && self.pending_link.is_none() => {
                self.current.push('_');
            }
            TagEnd::Strong if !self.in_code_block && self.pending_link.is_none() => {
                self.current.push('*');
            }
            TagEnd::Strikethrough if !self.in_code_block && self.pending_link.is_none() => {
                self.current.push('~');
            }
            TagEnd::Link => {
                if let Some(link) = self.pending_link.take() {
                    let text = if link.text.trim().is_empty() {
                        escape_markdown_v2_text(&link.dest)
                    } else {
                        link.text
                    };
                    if self.current.is_empty() && !self.in_code_block {
                        self.prefix();
                    }
                    self.current.push('[');
                    self.current.push_str(&text);
                    self.current.push_str("](");
                    self.current
                        .push_str(&escape_markdown_v2_link_dest(&link.dest));
                    self.current.push(')');
                }
            }
            _ => {}
        }
    }

    fn push_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if self.in_code_block {
            self.current.push_str(&escape_markdown_v2_code(text));
            return;
        }
        let escaped = escape_markdown_v2_text(text);
        if let Some(link) = self.pending_link.as_mut() {
            link.text.push_str(&escaped);
            return;
        }
        if self.current.is_empty() {
            self.prefix();
        }
        self.current.push_str(&escaped);
    }

    fn push_inline_code(&mut self, code: &str) {
        if let Some(link) = self.pending_link.as_mut() {
            link.text.push_str(&escape_markdown_v2_text(code));
            return;
        }
        if self.current.is_empty() && !self.in_code_block {
            self.prefix();
        }
        self.current.push('`');
        self.current.push_str(&escape_markdown_v2_code(code));
        self.current.push('`');
    }

    fn prefix(&mut self) {
        if self.blockquote_depth > 0 {
            for _ in 0..self.blockquote_depth {
                self.current.push_str("\\> ");
            }
        }
        if self.list_stack.len() > 1 {
            self.current
                .push_str(&"  ".repeat(self.list_stack.len().saturating_sub(1)));
        }
    }

    fn newline(&mut self) {
        if !self.current.is_empty() {
            self.lines.push(std::mem::take(&mut self.current));
        } else if self.lines.last().map(|s| !s.is_empty()).unwrap_or(true) {
            self.lines.push(String::new());
        }
    }

    fn finish(mut self) -> String {
        if !self.current.is_empty() {
            self.lines.push(self.current);
        }
        while self.lines.last().is_some_and(|s| s.is_empty()) {
            self.lines.pop();
        }
        self.lines.join("\n")
    }
}

fn sanitize_code_fence_lang(lang: &str) -> String {
    lang.chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '-'))
        .collect()
}

fn escape_markdown_v2_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if matches!(
            ch,
            '_' | '*'
                | '['
                | ']'
                | '('
                | ')'
                | '~'
                | '`'
                | '>'
                | '#'
                | '+'
                | '-'
                | '='
                | '|'
                | '{'
                | '}'
                | '.'
                | '!'
                | '\\'
        ) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

fn escape_markdown_v2_code(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if matches!(ch, '`' | '\\') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

fn escape_markdown_v2_link_dest(url: &str) -> String {
    let mut out = String::with_capacity(url.len());
    for ch in url.chars() {
        if matches!(ch, ')' | '\\') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

#[derive(Debug, Clone)]
struct TelegramAttachmentCandidate {
    kind: String,
    file_id: String,
    filename: Option<String>,
    mime: Option<String>,
    size_bytes: Option<u64>,
}

fn telegram_attachment_candidate(message: &Value) -> Option<TelegramAttachmentCandidate> {
    if let Some(photo_arr) = message
        .get("photo")
        .and_then(serde_json::Value::as_array)
        .filter(|arr| !arr.is_empty())
    {
        let best = photo_arr.iter().max_by_key(|item| {
            item.get("file_size")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
        })?;
        let file_id = best
            .get("file_id")
            .and_then(serde_json::Value::as_str)?
            .to_string();
        let size_bytes = best.get("file_size").and_then(serde_json::Value::as_u64);
        return Some(TelegramAttachmentCandidate {
            kind: "photo".to_string(),
            file_id,
            filename: None,
            mime: Some("image/jpeg".to_string()),
            size_bytes,
        });
    }

    let pick = |kind: &str, node: &Value| -> Option<TelegramAttachmentCandidate> {
        let file_id = node
            .get("file_id")
            .and_then(serde_json::Value::as_str)?
            .to_string();
        let filename = node
            .get("file_name")
            .and_then(serde_json::Value::as_str)
            .map(ToString::to_string);
        let mime = node
            .get("mime_type")
            .and_then(serde_json::Value::as_str)
            .map(ToString::to_string);
        let size_bytes = node.get("file_size").and_then(serde_json::Value::as_u64);
        Some(TelegramAttachmentCandidate {
            kind: kind.to_string(),
            file_id,
            filename,
            mime,
            size_bytes,
        })
    };

    if let Some(doc) = message.get("document").and_then(|v| pick("document", v)) {
        return Some(doc);
    }
    if let Some(v) = message.get("video").and_then(|v| pick("video", v)) {
        return Some(v);
    }
    if let Some(v) = message.get("animation").and_then(|v| pick("animation", v)) {
        return Some(v);
    }
    if let Some(v) = message.get("audio").and_then(|v| pick("audio", v)) {
        return Some(v);
    }
    if let Some(v) = message.get("voice").and_then(|v| pick("voice", v)) {
        return Some(v);
    }
    if let Some(v) = message
        .get("video_note")
        .and_then(|v| pick("video_note", v))
    {
        return Some(v);
    }
    if let Some(v) = message.get("sticker").and_then(|v| pick("sticker", v)) {
        return Some(v);
    }
    None
}

fn channel_uploads_root() -> PathBuf {
    let base = std::env::var("TANDEM_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            if let Some(data_dir) = dirs::data_dir() {
                return data_dir.join("tandem").join("data");
            }
            dirs::home_dir()
                .map(|home| home.join(".tandem").join("data"))
                .unwrap_or_else(|| PathBuf::from(".tandem"))
        });
    base.join("channel_uploads")
}

fn sanitize_filename(name: &str) -> String {
    let out = name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if out.is_empty() {
        "attachment.bin".to_string()
    } else {
        out
    }
}

pub struct TelegramChannel {
    bot_token: String,
    allowed_users: Vec<String>,
    mention_only: bool,
    style_profile: TelegramStyleProfile,
    client: Client,
    typing_handles: Arc<Mutex<std::collections::HashMap<String, JoinHandle<()>>>>,
}

impl TelegramChannel {
    pub fn new(config: TelegramConfig) -> Self {
        Self {
            bot_token: config.bot_token,
            allowed_users: config.allowed_users,
            mention_only: config.mention_only,
            style_profile: config.style_profile,
            client: Client::builder()
                .timeout(Duration::from_secs(35))
                .build()
                .expect("failed to create reqwest client"),
            typing_handles: Arc::new(Mutex::new(std::collections::HashMap::new())),
        }
    }

    fn api_url(&self, method: &str) -> String {
        format!("{}{}/{}", TELEGRAM_API, self.bot_token, method)
    }

    fn redact_token(&self, text: &str) -> String {
        text.replace(&format!("bot{}", self.bot_token), "bot<redacted>")
    }

    async fn bot_identity(&self) -> (Option<String>, Option<String>) {
        let resp = match self.client.get(self.api_url("getMe")).send().await {
            Ok(resp) => resp,
            Err(err) => {
                warn!("telegram getMe failed: {err}");
                return (None, None);
            }
        };
        let json: Value = match resp.json().await {
            Ok(json) => json,
            Err(err) => {
                warn!("telegram getMe parse failed: {err}");
                return (None, None);
            }
        };
        let result = json.get("result").cloned().unwrap_or_default();
        let username = result
            .get("username")
            .and_then(Value::as_str)
            .map(ToString::to_string);
        let id = result
            .get("id")
            .and_then(Value::as_i64)
            .map(|id| id.to_string());
        (username, id)
    }

    async fn download_telegram_attachment(
        &self,
        candidate: &TelegramAttachmentCandidate,
        chat_id: &str,
        update_id: i64,
    ) -> Option<String> {
        let max_bytes = std::env::var("TANDEM_CHANNEL_MAX_ATTACHMENT_BYTES")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(20 * 1024 * 1024);
        if candidate.size_bytes.is_some_and(|n| n > max_bytes) {
            warn!(
                "telegram attachment too large ({} bytes > {}), skipping download",
                candidate.size_bytes.unwrap_or(0),
                max_bytes
            );
            return None;
        }

        let get_file_resp = self
            .client
            .get(self.api_url("getFile"))
            .query(&[("file_id", candidate.file_id.as_str())])
            .send()
            .await
            .ok()?;
        if !get_file_resp.status().is_success() {
            return None;
        }
        let get_file_json: Value = get_file_resp.json().await.ok()?;
        let file_path = get_file_json
            .get("result")
            .and_then(|r| r.get("file_path"))
            .and_then(serde_json::Value::as_str)?;

        let file_url = format!(
            "https://api.telegram.org/file/bot{}/{}",
            self.bot_token, file_path
        );
        let file_resp = self.client.get(&file_url).send().await.ok()?;
        if !file_resp.status().is_success() {
            return None;
        }
        let bytes = file_resp.bytes().await.ok()?;
        if bytes.len() as u64 > max_bytes {
            warn!(
                "telegram attachment download exceeded max bytes ({} > {})",
                bytes.len(),
                max_bytes
            );
            return None;
        }

        let file_name = candidate.filename.clone().unwrap_or_else(|| {
            let ext = std::path::Path::new(file_path)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("bin");
            format!("{}_{}.{}", candidate.kind, update_id, ext)
        });
        let safe_name = sanitize_filename(&file_name);
        let dir = channel_uploads_root()
            .join("telegram")
            .join(sanitize_filename(chat_id));
        tokio::fs::create_dir_all(&dir).await.ok()?;

        let ts = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .ok()
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let path = dir.join(format!("{ts}_{safe_name}"));
        tokio::fs::write(&path, &bytes).await.ok()?;
        Some(path.to_string_lossy().to_string())
    }
}

fn strip_telegram_bot_mention(raw_text: &str, bot_username: Option<&str>) -> (String, bool) {
    let trimmed = raw_text.trim();
    let Some(bot_username) = bot_username.map(str::trim).filter(|s| !s.is_empty()) else {
        return (trimmed.to_string(), false);
    };
    let mention = format!("@{}", bot_username.trim_start_matches('@'));
    let mut parts = trimmed.splitn(2, char::is_whitespace);
    let first = parts.next().unwrap_or("");
    if first.eq_ignore_ascii_case(&mention) {
        let rest = parts.next().unwrap_or("").trim().to_string();
        return (rest, true);
    }
    (trimmed.to_string(), false)
}

#[async_trait]
impl Channel for TelegramChannel {
    fn name(&self) -> &str {
        "telegram"
    }

    async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
        let mut text_to_send = message.content.clone();

        for image_url in &message.image_urls {
            let photo_body = serde_json::json!({
                "chat_id": message.recipient,
                "photo": image_url,
            });
            let photo_resp = self
                .client
                .post(self.api_url("sendPhoto"))
                .json(&photo_body)
                .send()
                .await?;
            if !photo_resp.status().is_success() {
                let status = photo_resp.status();
                let err = photo_resp.text().await.unwrap_or_default();
                warn!("telegram sendPhoto failed ({status}) for url '{image_url}': {err}");
                if !text_to_send.is_empty() {
                    text_to_send.push('\n');
                }
                text_to_send.push_str(image_url);
            }
        }

        let styled = apply_telegram_style_profile(&text_to_send, self.style_profile);
        let formatted = format_markdown_for_telegram(&styled);
        for chunk in split_message(&formatted) {
            let markdown_body = serde_json::json!({
                "chat_id": message.recipient,
                "text": chunk,
                "parse_mode": "MarkdownV2",
            });
            let markdown_resp = self
                .client
                .post(self.api_url("sendMessage"))
                .json(&markdown_body)
                .send()
                .await?;

            if markdown_resp.status().is_success() {
                continue;
            }

            let markdown_status = markdown_resp.status();
            let markdown_error = markdown_resp.text().await.unwrap_or_default();
            warn!(
                "telegram sendMessage with MarkdownV2 failed ({markdown_status}); retrying plain text: {markdown_error}"
            );

            let plain_body = serde_json::json!({
                "chat_id": message.recipient,
                "text": chunk,
            });
            let plain_resp = self
                .client
                .post(self.api_url("sendMessage"))
                .json(&plain_body)
                .send()
                .await?;
            if !plain_resp.status().is_success() {
                let plain_status = plain_resp.status();
                let plain_error = plain_resp.text().await.unwrap_or_default();
                error!("telegram sendMessage plain text failed ({plain_status}): {plain_error}");
                anyhow::bail!("telegram sendMessage failed ({plain_status})");
            }
        }
        Ok(())
    }

    async fn listen(&self, tx: mpsc::Sender<ChannelMessage>) -> anyhow::Result<()> {
        let mut offset: i64 = 0;
        let (bot_username, bot_id) = self.bot_identity().await;
        loop {
            let resp = self
                .client
                .get(self.api_url("getUpdates"))
                .query(&[
                    ("timeout", "25"),
                    ("offset", &offset.to_string()),
                    ("allowed_updates", r#"["message"]"#),
                ])
                .send()
                .await;

            let resp = match resp {
                Ok(r) => r,
                Err(e) => {
                    let redacted = self.redact_token(&format!("{e:?}"));
                    warn!("telegram poll error: {redacted}");
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
            };

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                let preview = if body.chars().count() > 320 {
                    let truncated: String = body.chars().take(320).collect();
                    format!("{truncated}...")
                } else {
                    body
                };
                warn!("telegram getUpdates failed ({status}): {preview}");
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }

            let json: Value = match resp.json().await {
                Ok(v) => v,
                Err(e) => {
                    warn!("telegram json parse error: {e}");
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
            };

            let updates = match json.get("result").and_then(|r| r.as_array()) {
                Some(u) => u.clone(),
                None => {
                    debug!("telegram: no result array");
                    continue;
                }
            };

            for update in &updates {
                let update_id = update["update_id"].as_i64().unwrap_or(0);
                offset = offset.max(update_id + 1);

                let msg = match update.get("message") {
                    Some(m) => m,
                    None => continue,
                };

                let raw_text = msg
                    .get("text")
                    .and_then(|t| t.as_str())
                    .or_else(|| msg.get("caption").and_then(|t| t.as_str()))
                    .unwrap_or("");

                let chat_id = msg["chat"]["id"].as_i64().unwrap_or(0).to_string();
                let chat_type = msg["chat"]["type"].as_str().unwrap_or("");
                let message_thread_id = msg
                    .get("message_thread_id")
                    .and_then(Value::as_i64)
                    .map(|id| id.to_string());
                let attachment_candidate = telegram_attachment_candidate(msg);
                let (attachment, attachment_path, attachment_mime, attachment_filename) =
                    if let Some(candidate) = attachment_candidate {
                        let description = if candidate.kind == "document" {
                            candidate
                                .filename
                                .as_ref()
                                .map(|name| format!("document:{name}"))
                                .unwrap_or_else(|| "document".to_string())
                        } else {
                            candidate.kind.clone()
                        };
                        let stored = self
                            .download_telegram_attachment(&candidate, &chat_id, update_id)
                            .await;
                        (
                            Some(description),
                            stored,
                            candidate.mime,
                            candidate.filename,
                        )
                    } else {
                        (None, None, None, None)
                    };

                let username = msg["from"]["username"].as_str().map(|u| format!("@{u}"));
                let first_name = msg["from"]["first_name"].as_str().map(|n| n.to_string());
                let numeric_id = msg["from"]["id"].as_i64().map(|id| id.to_string());

                // Sender (display/trace identity) prefers @username, then first_name, then numeric ID.
                let sender = username
                    .clone()
                    .or_else(|| first_name.clone())
                    .or_else(|| numeric_id.clone())
                    .unwrap_or_else(|| "unknown".to_string());

                // Allow either username or numeric ID to match allowed_users.
                let allowed = if self.allowed_users.iter().any(|a| a == "*") {
                    true
                } else {
                    let candidates = [
                        username.as_deref(),
                        numeric_id.as_deref(),
                        Some(sender.as_str()),
                    ];
                    candidates
                        .iter()
                        .flatten()
                        .any(|candidate| is_user_allowed(candidate, &self.allowed_users))
                };

                if !allowed {
                    debug!("telegram: ignoring message from {sender} (not in allowed_users)");
                    continue;
                }

                let is_direct_message = chat_type == "private";
                let (content, was_explicitly_mentioned) =
                    strip_telegram_bot_mention(raw_text, bot_username.as_deref());
                let reply_from = msg
                    .get("reply_to_message")
                    .and_then(|reply| reply.get("from"));
                let reply_from_id = reply_from
                    .and_then(|from| from.get("id"))
                    .and_then(Value::as_i64)
                    .map(|id| id.to_string());
                let reply_from_username = reply_from
                    .and_then(|from| from.get("username"))
                    .and_then(Value::as_str);
                let is_reply_to_bot = reply_from
                    .and_then(|from| from.get("is_bot"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                    && (reply_from_id.as_deref() == bot_id.as_deref()
                        || reply_from_username
                            .zip(bot_username.as_deref())
                            .map(|(reply, bot)| reply.eq_ignore_ascii_case(bot))
                            .unwrap_or(false));
                let trigger = MessageTriggerContext {
                    source: if is_direct_message {
                        TriggerSource::DirectMessage
                    } else if was_explicitly_mentioned {
                        TriggerSource::Mention
                    } else if is_reply_to_bot {
                        TriggerSource::ReplyToBot
                    } else {
                        TriggerSource::Ambient
                    },
                    is_direct_message,
                    was_explicitly_mentioned,
                    is_reply_to_bot,
                };
                if !should_accept_message(
                    self.mention_only,
                    &trigger,
                    !content.is_empty(),
                    attachment.is_some(),
                ) {
                    continue;
                }
                let scope = if is_direct_message {
                    ConversationScope {
                        kind: ConversationScopeKind::Direct,
                        id: format!("dm:{chat_id}"),
                    }
                } else if let Some(thread_id) = message_thread_id {
                    ConversationScope {
                        kind: ConversationScopeKind::Topic,
                        id: format!("topic:{chat_id}:{thread_id}"),
                    }
                } else {
                    ConversationScope {
                        kind: ConversationScopeKind::Room,
                        id: format!("chat:{chat_id}"),
                    }
                };

                let channel_msg = ChannelMessage {
                    id: update_id.to_string(),
                    sender: sender.clone(),
                    reply_target: chat_id,
                    content,
                    channel: "telegram".to_string(),
                    timestamp: chrono::Utc::now(),
                    attachment,
                    attachment_url: None,
                    attachment_path,
                    attachment_mime,
                    attachment_filename,
                    trigger,
                    scope,
                };

                if tx.send(channel_msg).await.is_err() {
                    return Ok(()); // receiver dropped — shutdown
                }
            }
        }
    }

    async fn start_typing(&self, recipient: &str) -> anyhow::Result<()> {
        let url = self.api_url("sendChatAction");
        let body = serde_json::json!({ "chat_id": recipient, "action": "typing" });
        let client = self.client.clone();
        let recipient = recipient.to_string();
        let handle = tokio::spawn(async move {
            loop {
                let _ = client.post(&url).json(&body).send().await;
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        });
        self.typing_handles.lock().insert(recipient, handle);
        Ok(())
    }

    async fn stop_typing(&self, recipient: &str) -> anyhow::Result<()> {
        if let Some(handle) = self.typing_handles.lock().remove(recipient) {
            handle.abort();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_short_message() {
        let msg = "Hello, world!";
        assert_eq!(split_message(msg), vec![msg.to_string()]);
    }

    #[test]
    fn test_split_long_message() {
        let msg = "a".repeat(5000);
        let chunks = split_message(&msg);
        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert!(chunk.chars().count() <= MAX_MESSAGE_LEN);
        }
        assert_eq!(chunks.join(""), msg);
    }

    #[test]
    fn test_split_unicode_message() {
        let msg = "🦀 Rust ".repeat(1200);
        let chunks = split_message(&msg);
        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert!(chunk.chars().count() <= MAX_MESSAGE_LEN);
        }
        assert_eq!(chunks.join(""), msg);
    }

    #[test]
    fn test_split_prefers_safe_markdown_v2_boundary() {
        let bold = format!("*{}*", "x".repeat(4040));
        let msg = format!("{bold}\n{}", "y".repeat(300));
        let chunks = split_message(&msg);
        assert!(chunks.len() >= 2);
        assert!(chunks[0].ends_with('\n'));
        assert!(chunks[0].contains("*"));
    }

    #[test]
    fn test_markdown_v2_escapes_reserved_text_chars() {
        let out = format_markdown_for_telegram("Hello! (a+b) #1.");
        assert_eq!(out, "Hello\\! \\(a\\+b\\) \\#1\\.");
    }

    #[test]
    fn test_markdown_v2_preserves_basic_styles() {
        let out = format_markdown_for_telegram("**bold** _italic_ `x*y`");
        assert!(out.contains("*bold*"));
        assert!(out.contains("_italic_"));
        assert!(out.contains("`x*y`"));
    }

    #[test]
    fn test_markdown_v2_formats_links_with_safe_url() {
        let out = format_markdown_for_telegram("[Docs](https://example.com/a(b)\\\\c)");
        assert_eq!(out, "[Docs](https://example.com/a(b\\)\\\\c)");
    }

    #[test]
    fn test_markdown_v2_code_block_escapes_backticks_and_backslashes() {
        let out = format_markdown_for_telegram("```rust\nlet x = `a\\\\b`;\n```");
        assert!(out.contains("```rust"));
        assert!(out.contains("\\`a"));
        assert!(out.contains("b\\`"));
        assert!(out.ends_with("```"));
    }

    #[test]
    fn test_markdown_v2_headings_and_lists_render_as_safe_text() {
        let out = format_markdown_for_telegram("## Heading\n- item");
        assert!(out.contains("*Heading*"));
        assert!(out.contains("\\- item"));
    }

    #[test]
    fn test_telegram_style_profile_friendly_transforms_headings_and_lists() {
        let out = apply_telegram_style_profile(
            "## Updates\n- first\n1. second",
            TelegramStyleProfile::Friendly,
        );
        assert!(out.contains("✨ Updates"));
        assert!(out.contains("• first"));
        assert!(out.contains("• second"));
    }

    #[test]
    fn test_telegram_attachment_candidate_detects_photo() {
        let msg = serde_json::json!({
            "photo": [{ "file_id": "a", "file_size": 123 }]
        });
        let candidate = telegram_attachment_candidate(&msg).expect("candidate");
        assert_eq!(candidate.kind, "photo");
        assert_eq!(candidate.file_id, "a");
    }

    #[test]
    fn test_telegram_attachment_candidate_detects_document_with_name() {
        let msg = serde_json::json!({
            "document": { "file_id": "doc1", "file_name": "report.pdf", "mime_type": "application/pdf" }
        });
        let candidate = telegram_attachment_candidate(&msg).expect("candidate");
        assert_eq!(candidate.kind, "document");
        assert_eq!(candidate.file_id, "doc1");
        assert_eq!(candidate.filename.as_deref(), Some("report.pdf"));
    }
}
