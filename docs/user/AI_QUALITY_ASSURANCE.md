# AI Quality Assurance in Tandem

## What We Measure and Why

Tandem is an AI-powered automation platform. Like all AI systems, our AI components can fail in specific ways. We test for and measure these failures to ensure quality and safety.

This document explains:
- What quality assurance (QA) tests we run
- What metrics we track
- How we prevent regressions
- What happens when we find issues
- How you can trust Tandem with your work

## Core Quality Metrics

### Pass Rate: Success on Real-World Scenarios
**What it is**: The percentage of tests that complete successfully on the first or second attempt.

**Why it matters**: A higher pass rate means fewer failed automations, less manual intervention, and more reliable workflows.

**Our target**: >90% pass rate across all test scenarios.

**How we measure it**: We run 10+ test cases covering common automation patterns (research, code generation, document summarization, etc.) and track how many complete without errors.

### Average Repair Iterations: Self-Correction Ability
**What it is**: On average, how many times does Tandem re-attempt a task before it passes quality checks?

**Why it matters**: Fewer repair iterations = faster execution, lower cost, better user experience. If this number grows, it signals degraded AI reasoning.

**Our target**: <1.5 iterations on average (most tasks succeed on first attempt; some need 1-2 refinements).

**How we measure it**: When a task fails a quality check, our system automatically attempts repair (up to 3 times). We track the count per task.

### Cost Per Task: Efficiency
**What it is**: The estimated API costs for running one task end-to-end (from start to successful completion).

**Why it matters**: Higher costs = slower performance = less value for users. Cost increases signal inefficient token usage, more repair iterations, or model downgrades.

**Our target**: Stable cost per task (within 20% of baseline).

**How we measure it**: We multiply tokens used × model pricing and aggregate across all test runs.

### Provider Reliability: External Service Health
**What it is**: The percentage of API calls to our AI providers (Anthropic, OpenAI, etc.) that fail or timeout.

**Why it matters**: Provider issues are outside our control, but we monitor and handle them gracefully. High provider failure rates trigger fallback mechanisms.

**Our target**: <1% provider failure rate.

**How we measure it**: Every AI API call is tracked; failures (timeouts, rate limits, service errors) are counted and reported.

## Test Scenarios We Run

### Happy Path Tests (Priority 1: Core Functionality)
Basic, successful scenarios that users rely on daily:
- **Research task**: Retrieve and cite web sources
- **Code generation**: Write and validate working code
- **Document summarization**: Extract key points accurately
- **Multi-step workflow**: Chain multiple tasks together
- **Error recovery**: Handle and recover from transient failures

These tests must pass >95% of the time.

### Edge Cases (Priority 2: Robustness)
Unusual but realistic scenarios:
- Very long input documents
- Contradictory or ambiguous requirements
- Non-English text
- Rate-limited APIs
- Malformed data

These should pass >85% of the time.

### Regression Tests (Priority 3: Bug Prevention)
Tests for issues we've previously fixed. These ensure bugs don't reappear:
- **Citation validation**: Web sources must be cited with URLs
- **Fact checking**: Generated claims must be verifiable
- **Cost limits**: Tasks must respect budget constraints

These tests are critical; any failure triggers an incident review.

## How We Prevent Quality Degradation

### Automated Regression Detection
Every time code is deployed, we run our full test suite and compare results to a baseline. If metrics degrade beyond acceptable thresholds, the deployment is blocked:

- **Pass rate drops >5 percentage points** → Deployment blocked (critical)
- **Pass rate drops 2-5 percentage points** → Manual review required (warning)
- **Cost increases >20%** → Deployment blocked (critical)
- **Repair iterations increase >30%** → Manual review required (warning)

### Continuous Monitoring
We run evaluation tests:
- **On every PR**: Before code merges
- **Nightly**: Full test suite on latest main branch
- **Weekly**: Extended tests covering edge cases and slow scenarios
- **On-demand**: When investigating user-reported issues

### Failure Analysis
When tests fail, we automatically categorize failures:
- **Validation failures**: Output doesn't match expected structure
- **Provider failures**: API timeouts or errors
- **Resource failures**: Token budget or timeout exceeded
- **Data failures**: Retrieved information is corrupted or inaccessible
- **Authorization failures**: Invalid credentials or permissions

This taxonomy helps us identify root causes and fix them systematically.

## What We Test For

### Correctness
- Does the output match the specification?
- Are citations valid and accessible?
- Are facts verifiable?
- Does generated code run without errors?

### Safety
- Does the system respect safety constraints?
- Are harmful outputs rejected?
- Is user data handled correctly?
- Are API rate limits respected?

### Reliability
- Does the system recover from transient failures?
- Are timeout conditions handled gracefully?
- Can the system fall back to alternatives?
- Are errors logged and reported?

### Performance
- How long does a task take?
- How much does it cost?
- How many retries are needed?
- What's the memory/token efficiency?

## Transparency and Compliance

### EU AI Act Compliance (For European Users)
If you are in the EU, Tandem's AI quality assurance practices demonstrate compliance with the EU AI Act Article 50 transparency requirements:

✅ **Documented AI system**: Tandem's AI components are documented and tested.
✅ **Quality assurance**: This framework demonstrates systematic QA practices.
✅ **Failure categorization**: We categorize 30+ failure types for incident response.
✅ **Performance tracking**: Metrics are tracked before, during, and after deployment.
✅ **Regression prevention**: Automated gates prevent quality degradation.
✅ **Audit trail**: All test results are logged and timestamped for audit purposes.

We provide this documentation to natural persons interacting with our service (you) so you understand how AI quality is managed in the system you're using.

### Sharing Results With You
Upon request, we can provide:
- Summary of pass rates and costs for your specific workflows
- Failure analysis for tasks that failed repeatedly
- Historical trends in system quality over time
- Comparative performance across model variants

Contact support@tandem.local for detailed metrics.

## When Things Go Wrong

### What We Do
1. **Detect**: Automated tests catch regressions before deployment
2. **Alert**: Our team is notified of failures immediately
3. **Investigate**: Engineers categorize failures and identify root cause
4. **Resolve**: Code is fixed and re-tested before deployment
5. **Prevent**: Regression tests are added to prevent recurrence

### What You'll Experience
- **If caught before deployment**: You experience no impact; the buggy code never ships
- **If caught in production**: Your task may fail with a specific error message. The system will automatically retry using alternative approaches. If all retry attempts fail, you're notified with specific details.

### Reporting Issues
If you encounter repeated failures or unexpected behavior:
1. Note the task ID (provided in error messages)
2. Take a screenshot or log the error
3. Contact support@tandem.local with:
   - What you were trying to do
   - The error message or behavior
   - Approximate time it occurred
   - How many times you've seen this

Your report helps us identify patterns and prioritize fixes.

## Understanding Limitations

AI systems, including Tandem, have inherent limitations:

### What Tandem Does Well
- Routine research and fact-finding
- Code generation for common patterns
- Document summarization
- Multi-step workflow coordination
- Graceful failure recovery

### What Tandem Handles Carefully
- Specialized domain knowledge (may hallucinate)
- Novel or experimental tasks (less reliable)
- Adversarial inputs (may fail unexpectedly)
- Very long contexts (efficiency degrades)
- Real-time constraints (slower than alternatives)

### What Tandem Cannot Guarantee
- 100% accuracy (AI systems make mistakes)
- Perfect privacy (data is processed by external providers)
- Protection against adversarial inputs
- Constant availability (providers can have outages)

We're transparent about these limits in this documentation and your terms of service.

## Metrics Dashboard (Coming Soon)

Soon, you'll be able to view:
- Your personal workflow statistics (success rate, costs, speed)
- Comparative performance across different AI models
- Historical trends in your automation quality
- Specific failure categories from your runs

This will give you complete visibility into how Tandem is performing for your use cases.

## Trust and Verification

### How You Can Verify Our Claims
- **Audit our code**: Tandem's evaluation framework is open-source. You can review test definitions in `eval_datasets/*.yaml`
- **Run tests yourself**: The eval-runner CLI can be built and executed locally
- **Request audit results**: We can provide certified test results for compliance purposes
- **Independent review**: We welcome third-party security and AI safety audits

### Our Commitments
- We publish quality metrics regularly (monthly, minimum)
- We share incident reports when serious issues occur
- We maintain test coverage >85% for core functionality
- We never disable tests to hide regressions
- We follow industry-standard AI safety practices

## Contact & Support

For questions about AI quality assurance in Tandem:
- **General questions**: docs@tandem.local
- **Compliance/audit requests**: compliance@tandem.local
- **Bug reports**: support@tandem.local
- **Security concerns**: security@tandem.local

---

## Glossary

| Term | Meaning |
|------|---------|
| **Pass Rate** | Percentage of test cases that complete successfully |
| **Repair Iteration** | A retry attempt to fix a failed task |
| **Validation** | Quality check that output meets expected standards |
| **Regression** | Performance getting worse than baseline |
| **Failure Mode** | Categorization of why a task failed |
| **Provider** | External AI API (Anthropic, OpenAI, etc.) |
| **Token** | Unit of text used by AI models (roughly 4 characters per token) |
| **Threshold** | Limit beyond which an alert is triggered |

---

**Last updated**: May 15, 2026  
**Version**: 1.0  
**Framework version**: Phase 4 (Regression Detection)  

For the latest documentation, visit: https://docs.tandem.local/ai-quality-assurance
