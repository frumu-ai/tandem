const TOOL_CAPABLE_PATTERNS: RegExp[] = [
  /gpt/i,
  /claude/i,
  /gemini/i,
  /llama/i,
  /qwen/i,
  /deepseek/i,
  /mistral/i,
  /command/i,
  /o1/i,
  /o3/i,
  /o4/i,
  /glm/i,
  /grok/i,
];

export const isLikelyToolCapableModel = (modelId: string): boolean => {
  const normalized = modelId.trim();
  if (!normalized) return false;
  return TOOL_CAPABLE_PATTERNS.some((pattern) => pattern.test(normalized));
};

export const toolCapablePolicyReason = (modelId: string): string => {
  return `Model '${modelId}' is blocked by portal tool-capable policy. Pick a model known to support tool calls.`;
};
