use super::*;
use async_trait::async_trait;
use futures::{stream, Stream};
use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::Arc;
use tandem_providers::{ChatMessage, Provider, StreamChunk, TokenUsage};
use tandem_tools::Tool;
use tandem_types::{
    Message, MessagePart, MessageRole, ModelInfo, ProviderInfo, Session, ToolMode, ToolResult,
    ToolSchema,
};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

include!("integration_parts/helpers.rs");
include!("integration_parts/research_and_validation.rs");
include!("integration_parts/delivery_and_code_loop.rs");
include!("integration_parts/retries_and_recovery.rs");
