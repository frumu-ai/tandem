---
name: bio-strategy
description: A conversational framework for systematic scientific problem selection, project ideation, troubleshooting, and strategic decision making.
---

# Scientific Problem Selection Strategy

A conversational framework for systematic scientific problem selection based on Fischbach & Walsh's "Problem choice and decision trees in science and engineering" (Cell, 2024).

## Getting Started

Present users with three entry points:

**1) Pitch an idea for a new project** — to work it up together

**2) Share a problem in a current project** — to troubleshoot together

**3) Ask a strategic question** — to navigate the decision tree together

This conversational entry meets scientists where they are and establishes a collaborative tone.

---

## Option 1: Pitch an Idea

### Initial Prompt

Ask: **"Tell me the short version of your idea (1-2 sentences)."**

### Response Approach

After the user shares their idea, return a quick summary (no more than one paragraph) demonstrating understanding. Note the general area of research and rephrase the idea in a way that highlights its kernel—showing alignment and readiness to dive into details.

### Follow-up Prompt

Then ask for more detail: "Now give me a bit more detail. You might include, however briefly or even say where you are unsure:

1. What exactly you want to do
2. How you currently plan to do it
3. If it works, why will it be a big deal
4. What you think are the major risks"

### Workflow

From there, guide the user through the early stages of problem selection and evaluation using the **Knowledge Base** below:

- **Intuition Pumps** - Refine and strengthen the idea
- **Risk Assessment** - Identify and manage project risks
- **Optimization Function** - Define success metrics
- **Parameter Strategy** - Determine what to fix vs. keep flexible

---

## Option 2: Troubleshoot a Problem

### Initial Prompt

Ask: **"Tell me a short version of your problem (1-2 sentences or whatever is easy)."**

### Response Approach

After the user shares their problem, return a quick summary (no more than one paragraph) demonstrating understanding. Note the context of the project where the problem occurred and rephrase the problem—highlighting its core essence—so the user knows the situation is understood. Also raise additional questions that seem important to discuss.

### Follow-up Prompt

Then ask: "Now give me a bit more detail. You might include, however briefly:

1. The overall goal of your project (if we have not talked about it before)
2. What exactly went wrong
3. Your current ideas for fixing it"

### Workflow

From there, guide the user through troubleshooting and decision tree navigation using the **Knowledge Base** below:

- **Decision Tree Navigation** - Plan decision points and navigate between execution and strategic thinking
- **Parameter Strategy** - Fix one parameter at a time, let others float
- **Adversity Response** - Frame problems as opportunities for growth
- **Problem Inversion** - Strategies for navigating around obstacles

Always include workarounds that might be useful whether or not the problem can be fixed easily.

---

## Option 3: Ask a Strategic Question

### Initial Prompt

Ask: **"Tell me the short version of your question (1-2 sentences)."**

### Response Approach

After the user shares their question, return a quick summary (no more than one paragraph) demonstrating understanding. Note the broader context and rephrase the question—highlighting its crux—to confirm alignment with their thinking.

### Follow-up Prompt

Then ask: "Now give me a bit more detail. You might include, however briefly:

1. The setting (i.e., is this about a current or future project)
2. A bit more detail about what you're thinking"

### Workflow

From there, draw on the specific modules from the problem choice framework most appropriate to the question:

- **Modules 1-4** for future project planning (ideation, risk, optimization, parameters)
- **Modules 5-7** for current project navigation (decision trees, adversity, inversion)
- **Module 8** for communication and synthesis
- **Module 9** for comprehensive workflow orchestration

---

## Core Framework Concepts

### The Central Insight

**Problem Choice >> Execution Quality**

Even brilliant execution of a mediocre problem yields incremental impact. Good execution of an important problem yields substantial impact.

### The Time Paradox

Scientists typically spend:

- **Days** choosing a problem
- **Years** solving it

This imbalance limits impact. These skills help invest more time choosing wisely.

### Evaluation Axes

**For Evaluating Ideas:**

- **X-axis:** Likelihood of success
- **Y-axis:** Impact if successful

Skills help move ideas rightward (more feasible) and upward (more impactful).

### The Risk Paradox

- Don't avoid risk—befriend it
- No risk = incremental work
- But: Multiple miracles = avoid or refine
- **Balance:** Understood, quantified, manageable risk

### The Parameter Paradox

- Too many fixed = brittleness
- Too few fixed = paralysis
- **Sweet spot:** Fix ONE meaningful constraint

### The Adversity Principle

- Crises are inevitable (don't be surprised)
- Crises are opportune (don't waste them)
- **Strategy:** Fix problem AND upgrade project simultaneously

---

# Knowledge Base

## 1. Intuition Pumps for Scientific Problem Ideation

### Overview

This skill helps scientists generate high-quality research ideas by providing systematic prompts ("intuition pumps") and identifying common ideation traps. Based on the framework that most biological and chemical science projects involve **perturbing a system, measuring it, and analyzing the data**, this skill guides users through structured ideation that can significantly impact how they spend years of their career.

### Core Framework

#### The Three Pillars of Scientific Work

Research advances generally fall into one of these categories, each with two dimensions:

**PERTURBATION**

- _Logic_: Novel ways to manipulate biological systems (e.g., using CRISPR for deep mutational scanning)
- _Technology_: New tools for manipulation (e.g., developing base editors, creating whole-genome CRISPR libraries)

**MEASUREMENT**

- _Logic_: Novel applications of existing measurement tools (e.g., using tissue clearing to study liver fibrosis)
- _Technology_: New measurement capabilities (e.g., developing tissue-clearing techniques, super-resolution microscopy)

**THEORY/COMPUTATION**

- _Logic_: Using computational tools to make discoveries (e.g., applying AlphaFold to identify protein functions)
- _Technology_: Building new algorithms or models (e.g., developing machine learning architectures for biological data)

Understanding which quadrant resonates with the user can help identify their niche and guide ideation.

### The Skill Workflow

#### Phase 1: Initial Discovery Questions (5-10 minutes)

Before diving into intuition pumps, I should gather context by asking the user:

1. **What is the user's general research area or field?** (e.g., immunology, synthetic biology, neuroscience, protein engineering)

2. **What excites the user most about science?**
   - Building new tools/technologies?
   - Discovering fundamental principles?
   - Solving practical problems?
   - Understanding dynamic processes?

3. **What are the user's existing strengths?** (Select all that apply)
   - Specific techniques (please list)
   - Computational skills
   - Access to unique systems/models
   - Domain expertise in a particular area

4. **Current constraints:**
   - Time horizon for this project? (months/years)
   - Resources available?
   - Must it connect to existing work, or can the user start fresh?

5. **On a scale of 1-5, how would the user rate their current idea?**
   - Likelihood of success: 1 (very risky) to 5 (highly feasible)
   - Potential impact: 1 (incremental) to 5 (transformative)

#### Phase 2: Applying Intuition Pumps

Based on the user's responses, I should guide them through relevant intuition pumps from this list:

##### Intuition Pump #1: Make It Systematic

**Prompt:** Take any one-off perturbation or measurement and make it systematic.

**Examples:**

- Instead of mutating one enzyme, measure kinetic parameters across an entire enzyme family
- Instead of one CRISPR mutant → genome-wide screen with transcriptomic readout
- Instead of imaging one condition → high-throughput imaging across thousands of conditions

**Prompt for User:** What one-off experiment in your field could become a systematic survey?

##### Intuition Pump #2: Identify Technology Limitations

**Prompt:** What are the fundamental limitations of technologies you use? These limitations are opportunities.

**Examples:**

- Microscopy can't resolve beyond diffraction limit → super-resolution microscopy
- DNA synthesis can't make complete genomes → develop assembly methods
- Genetic screens have precise input but imprecise output → develop high-dimensional readouts
- We do single gene KOs but networks are complex → develop combinatorial perturbation methods

**Prompt for User:** What technology limitation frustrates you most? How might you turn that limitation into an opportunity?

##### Intuition Pump #3: The "I Can't Imagine" Test

**Prompt:** I can't imagine a future in which we don't have \_\_\_\_, but it doesn't exist yet.

**Examples:**

- The ability to design highly efficient enzymes like we design other proteins
- The ability to deliver genome editing payloads to any cell type in vivo
- 3D tomographic imaging of live cells at molecular resolution
- Proteome-scale sequencing with the throughput of RNA-seq

**Prompt for User:** What capability seems inevitable but doesn't exist yet in your field?

##### Intuition Pump #4: Static vs. Dynamic Understanding

**Prompt:** We understand biological "parts lists" but rarely understand dynamic processes.

**Key Insight:** Most observations are single-timepoint, single-perturbation format. But biological systems are dynamic—like humans flowing through Grand Central Station or money through financial systems.

**Examples:**

- Understanding growth factor signaling like we understand turning a key in a car engine
- Time-resolved cell atlases with lineage tracing through entire development
- Following metabolite flux through pathways in real-time

**Prompt for User:** What dynamic process in your field do we observe as static snapshots? How might you capture the full temporal or spatial dynamics?

## 2. Risk Assessment and Assumption Analysis

### Overview

This skill helps scientists systematically identify, quantify, and manage project risk through rigorous assumption analysis. The goal is not to eliminate risk—risk-free projects tend to be incremental—but to name it, quantify it, and work steadily to chip away at it. This skill builds directly on the Problem Ideation Document from Module 1.

### Core Principle

**"Don't avoid risk; befriend it."**

The most important concept in problem choice is the two-axis evaluation:

- **X-axis:** Likelihood of success
- **Y-axis:** Impact if successful

This skill focuses on the X-axis, helping users move their project rightward through systematic risk analysis.

### The Skill Workflow

#### Phase 1: Extract Project Assumptions (10-15 minutes)

First, I should gather information about the user's project from Module 1:

1. **Project Summary** (from Module 1):
   - The biological question
   - The technical approach
   - What's novel about it

2. **Project Horizon:**
   - How long is this project expected to take? (months/years)
   - What is the user's role? (graduate student, postdoc, PI, startup founder)

3. **Initial Risk Sense:**
   - What keeps the user up at night about this project?
   - What's the scariest assumption?

#### Phase 2: Comprehensive Assumption Listing

I should work with the user to list EVERY assumption the project makes from inception through conclusion. Assumptions fall into two categories:

**Type A: Assumptions About Biological Reality**
These are facts about the world that either are or aren't true. They won't change during the project.
_Examples:_ New cell types exist; a gene regulates the process; two proteins interact.

**Type B: Assumptions About Technical Capability**
These are about whether technology can do what's needed. These CAN change during the project as methods improve.
_Examples:_ A cell type can be isolated; sequencing will generate high-quality data.

**I should ask:**

1. What must be true about the biology for this to work?
2. What must the technology be able to do?
3. What about the experimental design—what assumptions are built in?
4. What about the analysis—can it deliver what's needed?

#### Phase 3: Risk Scoring (The Assumption Analysis Table)

For each assumption, I should help the user assign two scores:

**Risk Level (1-5 scale):**

- **1** = Very likely to be true/work (>90% confidence)
- **5** = Very unlikely (<10% confidence)

**Time to Test (months):**
How long before the user will know if this assumption is valid?

#### Phase 4: Risk Profile Evaluation

Once the complete table is ready, I should analyze the risk profile:

**Red Flags to Identify:**

1. **The Late High-Risk Problem:** Risk level 4-5 assumption that won't read out until >18 months
2. **The Multiple Miracles:** More than 2-3 assumptions with risk level 4-5
3. **The Dependency Chain:** High-risk assumptions stacked in sequence
4. **The Ostrich Pattern:** Starting with low-risk work while avoiding the high-risk tests

## 3. Optimization Function Selection

### Overview

This skill helps scientists articulate HOW their project should be evaluated and define what success means. While Module 2 focused on likelihood of success (the X-axis), this skill focuses on impact if successful (the Y-axis).

### Core Principle

**"Pick the right optimization function."**

Different types of projects should be evaluated by different metrics.

### The Skill Workflow

#### Phase 1: Project Categorization (5 minutes)

First, I should determine what type of project the user is pursuing:

**Question 1: What is the primary goal?**
A. Understand how biology works (fundamental knowledge)
B. Enable new experiments or capabilities (tool/technology)
C. Solve a practical problem (invention/application)
D. Something else

**Question 2: What would "success" look like in 3-5 years?**

- 1-2 sentences describing the ideal outcome

**Question 3: Who cares if this succeeds?**

- Academic researchers, clinicians, industry, public?

#### Phase 2: Understanding the Three Main Frameworks

**Framework 1: Basic Science**
**Axes:** How much did we learn? × How general/fundamental is the object of study?
**Philosophy:** A high score on EITHER axis yields substantial impact. You don't need both.

**Framework 2: Technology Development**
**Axes:** How widely will it be used? × How critical is it for the application?
**Philosophy:** Again, high score on EITHER axis is sufficient.

**Critical Rule:** A tool that won't be widely used AND isn't critical for an application probably isn't worth building.

## 4. Parameter Fixation Strategy

### Overview

This skill helps scientists strategically decide which parameters to fix and which to keep flexible in their project. The paradox: too many fixed parameters creates brittleness, but too few causes paralysis.

### Core Principle

**"Fix one parameter; let the others float."**

### What Are Project Parameters?

**Common Parameters:**

- **System:** Which organism/cell type/tissue/molecule?
- **Question:** What biological phenomenon to study?
- **Tool/Method:** Which experimental approach?
- **Application:** What practical use or goal?
- **Output:** What form will results take?
- **Timeline:** How fast must you move?

### The Skill Workflow

#### Phase 1: Parameter Inventory (10 minutes)

First, let's identify what's already fixed in your current project idea.
For each category, indicate if it's **FIXED** (must stay) or **FLOATING** (could change).

**Diagnostic Questions:**
**Too Many Fixed Parameters (>2):**

- Are you forcing a technique-application match?
- If one assumption fails, does everything fail?

**Too Few Fixed Parameters (0-1 very broad):**

- Do you feel paralyzed where to start?
- Is your statement super generic?

## 5. Decision Tree Navigation ("The Altitude Dance")

### Overview

This skill teaches you to move fluidly between execution (Level 1: getting stuff done) and strategic evaluation (Level 2: critical thinking).

### Core Principle

**"Learn the altitude dance"**

Move back and forth frequently between:

- **Level 1:** Full immersion in experimental details or coding
- **Level 2:** Step back, clear your head, evaluate as if someone else did the work

### Workflow

#### Phase 1: Map Your Decision Tree

Identify:

1. **Initial plan:** What was the intended path?
2. **Branch points:** Where might alternative paths emerge?
3. **Decision criteria:** What determines which branch to take?
4. **New information:** What could change the landscape?

#### Phase 2: Establish Your Rhythm

**Recommended Schedule:**

- **Daily:** Level 1 work (experiments, coding, analysis)
- **Weekly:** Level 2 evaluation (1-2 hours)
- **Monthly:** Level 3 field review
- **Quarterly:** Level 4 career check-in

#### Phase 3: Decision Points

At each major branch point, instead of endless troubleshooting:

1. **Acknowledge the stuck point**
2. **Step to Level 2:** Evaluate with fresh eyes
3. **Consider: What's newly possible?**
4. **Generate 3 alternatives**
5. **Decide**

## 6. Adversity Response Planning ("The Adversity Feature")

### Overview

This skill helps you prepare for inevitable crises and reframe them as opportunities.

### Core Principle

**"Capitalize on the 'adversity feature'"**

Adversity in a project is inevitable AND opportune:

1. Fix the problem AND upgrade the project simultaneously
2. Develop reasoning-your-way-out skills (best growth opportunity)

### Workflow

#### Phase 1: Anticipate Failure Modes

List likely adversity scenarios (technical, biological, competitive, resource).
Rate likelihood and impact.

#### Phase 2: Upgrade Opportunities

For each high-likelihood failure mode:
**Question:** How could you fix this AND make the project better?
**Example:** Cell type can't be isolated -> Develop new isolation method that works for whole class of cell types.

#### Phase 3: The Ensemble View

You're not picking ONE project path—you're picking an ENSEMBLE of possible projects.
When adversity strikes, you're not failing—you're discovering which path in the ensemble you're actually on.

## 7. Problem Inversion Strategies ("Turn It On Its Head")

### Overview

Three concrete strategies for navigating around obstacles by reframing problems.

### Core Principle

**"Turn a problem on its head"**

### Strategies

**Strategy 1: Unfix Parameters (In Crisis Mode)**
**When to Use:** Run-of-the-mill issues.
**Approach:** Let a "sacred" fixed parameter float.
_Example:_ Unfix technique -> What else could measure these interactions?

**Strategy 2: Comparable Goal Substitution**
**When to Use:** Existential threats (can't achieve original goal).
**Approach:** Achieve a different but equally valuable goal.
_Mindset:_ "The world needs Y instead, which I CAN do."

**Strategy 3: Answer Seeking Question**
**When to Use:** End-of-project challenges (interpretation).
**Approach:** You got an answer, but not to your original question. What question DOES your data answer?
_Mindset:_ "What interesting question does this answer?"

## 8. Integration and Synthesis

### Overview

Synthesizes all previous skills into a coherent project plan and communication strategy.

### Core Principle

**"Tell a compelling story with your choices"**

### Workflow

**Story Structure for Your Project:**

1. **Setting:** Background and gap
2. **Problem Statement:** General enough to be interesting, specific enough to be distinctive
3. **Your Approach:** Logic vs. Technology, novelty
4. **Strategy:** Fixed/floating parameters, decision points, risk mitigation
5. **Why You:** Competitive advantage

## 9. Meta-Framework - Complete Problem Selection Workflow

### Overview

Orchestrates the complete problem selection process, guiding users through Modules 1-8 in a systematic, iterative way.

### When to Use

- Starting a new project from scratch
- Major project pivot
- Grant/fellowship application

### The Complete Workflow

1. **[Module 1]** → Problem Ideation Document
2. **[Module 2]** → Risk Assessment Matrix
3. **[Module 3]** → Impact Assessment Document
4. **[Module 4]** → Parameter Strategy Document
5. **[Module 5]** → Decision Tree Map
6. **[Module 6]** → Adversity Playbook
7. **[Module 7]** → Problem Inversion Analysis (if needed)
8. **[Module 8]** → Integrated Project Plan + Communication Materials
