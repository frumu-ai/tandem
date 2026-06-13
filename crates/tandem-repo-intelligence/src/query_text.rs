#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NormalizedText {
    raw: String,
    tokens: Vec<String>,
    compact: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MatchScore {
    pub tier: u8,
    pub missed_tokens: usize,
    pub token_hits: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TokenMatchMode {
    RequireAll,
    AllowPartial,
}

impl NormalizedText {
    pub(crate) fn for_query(value: &str) -> Self {
        let mut normalized = normalize(value);
        let filtered = normalized
            .tokens
            .iter()
            .filter(|token| !STOP_WORDS.contains(&token.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        if !filtered.is_empty() {
            normalized.tokens = filtered;
            normalized.compact = normalized.tokens.join("");
        }
        normalized
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.raw.trim().is_empty() && self.tokens.is_empty()
    }

    pub(crate) fn display(&self) -> String {
        if self.tokens.is_empty() {
            self.raw.clone()
        } else {
            self.tokens.join(" ")
        }
    }
}

pub(crate) fn normalize(value: &str) -> NormalizedText {
    let raw = value.trim().to_ascii_lowercase();
    let tokens = tokenize(value);
    let compact = tokens.join("");
    NormalizedText {
        raw,
        tokens,
        compact,
    }
}

pub(crate) fn match_text(
    query: &NormalizedText,
    target: &str,
    mode: TokenMatchMode,
) -> Option<MatchScore> {
    if query.is_empty() {
        return Some(MatchScore {
            tier: 9,
            missed_tokens: 0,
            token_hits: 0,
        });
    }

    let target = normalize(target);
    if !query.raw.is_empty() && target.raw == query.raw {
        return Some(MatchScore {
            tier: 0,
            missed_tokens: 0,
            token_hits: query.tokens.len(),
        });
    }
    if !query.compact.is_empty() && target.compact == query.compact {
        return Some(MatchScore {
            tier: 1,
            missed_tokens: 0,
            token_hits: query.tokens.len(),
        });
    }
    if !query.raw.is_empty() && target.raw.contains(&query.raw) {
        return Some(MatchScore {
            tier: 2,
            missed_tokens: 0,
            token_hits: query.tokens.len(),
        });
    }
    if !query.compact.is_empty() && target.compact.contains(&query.compact) {
        return Some(MatchScore {
            tier: 3,
            missed_tokens: 0,
            token_hits: query.tokens.len(),
        });
    }

    let token_hits = query
        .tokens
        .iter()
        .filter(|token| target.tokens.iter().any(|candidate| candidate == *token))
        .count();
    if token_hits == 0 {
        return None;
    }
    let missed_tokens = query.tokens.len().saturating_sub(token_hits);
    let accepts_partial = match mode {
        TokenMatchMode::RequireAll => missed_tokens == 0,
        TokenMatchMode::AllowPartial => accepts_partial_token_match(query, token_hits),
    };
    accepts_partial.then_some(MatchScore {
        tier: 4,
        missed_tokens,
        token_hits,
    })
}

pub(crate) fn best_match(
    query: &NormalizedText,
    targets: &[&str],
    mode: TokenMatchMode,
) -> Option<MatchScore> {
    targets
        .iter()
        .filter_map(|target| match_text(query, target, mode))
        .min_by(compare_match_score)
}

pub(crate) fn query_terms(value: &str, limit: usize) -> Vec<String> {
    let mut terms = Vec::new();
    for token in NormalizedText::for_query(value).tokens {
        if token.len() < 3 || terms.contains(&token) {
            continue;
        }
        terms.push(token);
        if terms.len() >= limit {
            break;
        }
    }
    terms
}

pub(crate) fn compare_match_score(left: &MatchScore, right: &MatchScore) -> std::cmp::Ordering {
    left.tier
        .cmp(&right.tier)
        .then(left.missed_tokens.cmp(&right.missed_tokens))
        .then(right.token_hits.cmp(&left.token_hits))
}

fn accepts_partial_token_match(query: &NormalizedText, token_hits: usize) -> bool {
    if query.tokens.len() <= 1 {
        return true;
    }
    token_hits >= 2
        || query
            .tokens
            .iter()
            .any(|token| token.len() >= 8 && token_hits >= 1)
}

fn tokenize(value: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut previous: Option<char> = None;
    for ch in value.chars() {
        if !ch.is_ascii_alphanumeric() {
            push_token(&mut tokens, &mut current);
            previous = None;
            continue;
        }
        if ch.is_ascii_uppercase()
            && previous.is_some_and(|prev| prev.is_ascii_lowercase() || prev.is_ascii_digit())
        {
            push_token(&mut tokens, &mut current);
        }
        current.push(ch.to_ascii_lowercase());
        previous = Some(ch);
    }
    push_token(&mut tokens, &mut current);
    tokens
}

fn push_token(tokens: &mut Vec<String>, current: &mut String) {
    if !current.is_empty() {
        tokens.push(std::mem::take(current));
    }
}

const STOP_WORDS: &[&str] = &[
    "a",
    "an",
    "and",
    "are",
    "can",
    "does",
    "for",
    "from",
    "how",
    "implemented",
    "into",
    "is",
    "of",
    "on",
    "the",
    "this",
    "that",
    "to",
    "what",
    "where",
    "with",
];
