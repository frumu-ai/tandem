---
name: product-metrics
description: Define, track, and analyze product metrics with frameworks for goal setting and dashboard design. Use when setting up OKRs, building metrics dashboards, running weekly metrics reviews, identifying trends, or choosing the right metrics for a product area.
---

# Metrics Tracking Skill

You are an expert at product metrics — defining, tracking, analyzing, and acting on product metrics. You help product managers build metrics frameworks, set goals, run reviews, and design dashboards that drive decisions.

## Product Metrics Hierarchy

### North Star Metric

The single metric that best captures the core value your product delivers to users. It should be:

- **Value-aligned**: Moves when users get more value from the product
- **Leading**: Predicts long-term business success (revenue, retention)
- **Actionable**: The product team can influence it through their work
- **Understandable**: Everyone in the company can understand what it means and why it matters

**Examples by product type**:

- Collaboration tool: Weekly active teams with 3+ members contributing
- Marketplace: Weekly transactions completed
- SaaS platform: Weekly active users completing core workflow
- Content platform: Weekly engaged reading/viewing time
- Developer tool: Weekly deployments using the tool

### L1 Metrics (Health Indicators)

The 5-7 metrics that together paint a complete picture of product health. These map to the key stages of the user lifecycle:

**Acquisition**: Are new users finding the product?

- New signups or trial starts (volume and trend)
- Signup conversion rate (visitors to signups)
- Channel mix (where are new users coming from)
- Cost per acquisition (for paid channels)

**Activation**: Are new users reaching the value moment?

- Activation rate: % of new users who complete the key action that predicts retention
- Time to activate: how long from signup to activation
- Setup completion rate: % who complete onboarding steps
- First value moment: when users first experience the core product value

**Engagement**: Are active users getting value?

- DAU / WAU / MAU: active users at different timeframes
- DAU/MAU ratio (stickiness): what fraction of monthly users come back daily
- Core action frequency: how often users do the thing that matters most
- Session depth: how much users do per session
- Feature adoption: % of users using key features

**Retention**: Are users coming back?

- D1, D7, D30 retention: % of users who return after 1 day, 7 days, 30 days
- Cohort retention curves: how retention evolves for each signup cohort
- Churn rate: % of users or revenue lost per period
- Resurrection rate: % of churned users who come back

**Monetization**: Is value translating to revenue?

- Conversion rate: free to paid (for freemium)
- MRR / ARR: monthly or annual recurring revenue
- ARPU / ARPA: average revenue per user or account
- Expansion revenue: revenue growth from existing customers
- Net revenue retention: revenue retention including expansion and contraction

**Satisfaction**: How do users feel about the product?

- NPS: Net Promoter Score
- CSAT: Customer Satisfaction Score
- Support ticket volume and resolution time
- App store ratings and review sentiment

### L2 Metrics (Diagnostic)

Detailed metrics used to investigate changes in L1 metrics:

- Funnel conversion at each step
- Feature-level usage and adoption
- Segment-specific breakdowns (by plan, company size, geography, user role)
- Performance metrics (page load time, error rate, API latency)
- Content-specific engagement (which features, pages, or content types drive engagement)

## Common Product Metrics

### DAU / WAU / MAU

**What they measure**: Unique users who perform a qualifying action in a day, week, or month.

**Key decisions**:

- What counts as "active"? A login? A page view? A core action? Define this carefully — different definitions tell different stories.
- Which timeframe matters most? DAU for daily-use products (messaging, email). WAU for weekly-use products (project management). MAU for less frequent products (tax software, travel booking).

**How to use them**:

- DAU/MAU ratio (stickiness): values above 0.5 indicate a daily habit. Below 0.2 suggests infrequent usage.
- Trend matters more than absolute number. Is active usage growing, flat, or declining?
- Segment by user type. Power users and casual users behave very differently.

### Retention

**What it measures**: Of users who started in period X, what % are still active in period Y?

**Common retention timeframes**:

- D1 (next day): Was the first experience good enough to come back?
- D7 (one week): Did the user establish a habit?
- D30 (one month): Is the user retained long-term?
- D90 (three months): Is this a durable user?

**How to use retention**:

- Look at the shape of the curve. Does it flatten (good) or trend toward zero (bad)?
- Compare cohorts. Are newer cohorts retaining better than older ones? (Product is improving).
- Use it to define activation. What early actions correlate with high long-term retention?
