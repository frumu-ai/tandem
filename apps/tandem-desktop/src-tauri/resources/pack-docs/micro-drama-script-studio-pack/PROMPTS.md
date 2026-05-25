# Tandem Prompts - Micro-Drama Script Studio

Copy and paste these prompts sequentially into Tandem. Each prompt builds on the previous outputs.

---

## Prompt 1: Workspace Scan & Constraint Extraction

```
You are a micro-drama script analyst. Your task is to scan all files in the inputs/ directory and produce a comprehensive summary.

Read ALL files in inputs/ including:
- STYLE_GUIDE.md
- SCRIPT_FORMAT.md
- CHARACTER_SHEETS.md
- All files in EXAMPLE_EPISODES/
- All files in TOPIC_GUIDES/

Create a summary document (outputs/workspace_scan.md) that includes:

## Available Reference Materials
- List each file with a 1-sentence description of its purpose

## Tone & Style Guidelines
- Primary emotional tone(s)
- Target episode length
- Pacing characteristics
- Voice/style requirements

## Script Format Requirements
- Scene heading format
- Character name conventions
- Dialogue formatting
- Action line style
- Any special notations

## Content Constraints
- Topics to avoid
- Content sensitivities
- Platform requirements

## Example Analysis
- What makes the example episodes effective?
- Key patterns in dialogue and structure
- Common phrases or tropes

Save your summary to outputs/workspace_scan.md and explain your reasoning before writing.
```

---

## Prompt 2: Writer's Room - Premise Development

```
You are a micro-drama showrunner. Using the style guidelines and constraints from inputs/, develop three distinct episode premises.

For EACH premise, provide:

## Premise Option [1/2/3]
**Logline**: One sentence hook (max 20 words)

**Genre**: [e.g., Romance, Thriller, Comedy, Drama, Sci-Fi]

**Setting**: Where does this story take place?

**Core Conflict**: What is the central tension or problem?

**Character Pairing**:
- Character A: [Brief description and motivation]
- Character B: [Brief description and motivation]

**Episode Hook**: What creates the initial intrigue (first 10 seconds)?

**Cliffhanger**: What leaves viewers wanting the next episode?

**Episode Structure**:
- Scene 1 (0-30s): [Setup]
- Scene 2 (30-60s): [Confrontation/Complication]
- Scene 3 (60-90s): [Revelation/Twist]
- Scene 4 (90-120s): [Cliffhanger]

**Tone**: [e.g., Tense, Romantic, Humorous, Melancholic]

**Tropes**: List 2-3 genre tropes used

---

After presenting all three options, recommend ONE option and explain why it best fits the style guidelines. Ask me to choose an option, or proceed with the recommendation if I approve.

Save the premises document to outputs/premises_options.md
```

---

## Prompt 3: Episode Beats & Outline

```
Using the selected premise [INSERT PREMISE NUMBER], create a detailed episode beat sheet.

## Episode Overview
- **Working Title**: [Catchy 3-5 word title]
- **Episode Number**: 001
- **Target Runtime**: [60-180 seconds]
- **Total Scenes**: [3-5 scenes]
- **Characters**: List all characters with brief descriptions

## Scene-by-Scene Beats

### Scene 1: [Title]
- **Duration**: 0-30s
- **Location**: [Setting]
- **Characters Present**: [Who appears]
- **Purpose**: [What this scene accomplishes]
- **Visual Hook**: [Eye-catching visual element]
- **Dialogue Start**: [Opening line suggestion]

### Scene 2: [Title]
- **Duration**: 30-60s
- **Location**: [Setting]
- **Characters Present**: [Who appears]
- **Purpose**: [What this scene accomplishes]
- **Key Moment**: [The scene's highlight]
- **Dialogue Start**: [Opening line suggestion]

[Continue for all scenes...]

### Final Scene: [Title]
- **Duration**: 90-120s or 120-150s
- **Location**: [Setting]
- **Characters Present**: [Who appears]
- **Purpose**: [Set up episode end and cliffhanger]
- **Cliffhanger Element**: [What creates the hook for next episode]

## Pacing Notes
- Rhythm of dialogue vs. action
- Where to accelerate
- Where to pause for impact

## Dialogue Guidelines
- Speech patterns for each character
- Key lines to hit
- Phrases to avoid

## Arc Notes
- How this episode fits the larger story
- Character development moments
- Setup for future episodes

Save to outputs/episode_beats_001.md
```

---

## Prompt 4: Script Draft Generation

```
Write a complete micro-drama episode script based on the beats outline in outputs/episode_beats_001.md.

## Formatting Requirements
Follow the script format from inputs/SCRIPT_FORMAT.md exactly. Use the character naming convention (e.g., A:, B:, M:) consistently.

## Script Structure

### HEADER
```

Episode: 001
Title: [Episode Title]
Runtime: [XX:XX]
Genre: [Genre]
Characters: [List]

```

### SCENES
Format each scene with:
- Scene heading in CAPS
- Brief action lines (present tense, visual)
- Character names in CAPS followed by dialogue
- [beat] notations for pauses
- Parentheticals sparingly for context

### DIALOGUE NOTES
- Keep dialogue punchy and short
- Show emotion through action, not just words
- Each character should have a distinct voice
- Avoid on-the-nose exposition

### RUNTIME CHECK
- Aim for target runtime
- Mark estimated timestamps for each scene
- Ensure cliffhanger lands in final 15 seconds

## Quality Standards
- Every scene advances plot or character
- Dialogue reveals character and advances conflict
- Visual storytelling in action lines
- Clear emotional arc within episode
- Strong cliffhanger that demands a follow-up

## Output Format
Save to: outputs/episode_001.md

Before writing, explain your approach to:
- How you'll handle the opening hook
- The emotional throughline
- How you'll make the cliffhanger effective

Write the script after I approve your approach.
```

---

## Prompt 5: HTML Writer's Room Dashboard

```
Create a comprehensive HTML dashboard artifact that summarizes the entire episode development process.

## Dashboard Sections Required

### 1. Episode Header
- Episode number and title
- Genre and runtime
- Creation date
- Version number

### 2. Premise Card
- Logline (prominently displayed)
- Genre classification
- Core conflict summary
- Tone indicators

### 3. Cast Sheet
- Character list with:
  - Name and role
  - Brief description
  - Personality traits
  - Character arc notes

### 4. Episode Beats Dashboard
- Visual scene breakdown (cards or timeline)
- Each scene with:
  - Scene number
  - Location
  - Duration
  - Key action point
  - Dialogue highlight

### 5. Arc Tracking
- This episode's role in larger story
- Character development markers
- Foreshadowing elements
- Callbacks to earlier episodes

### 6. Production Checklist
- [ ] Script complete
- [ ] Dialogue recorded
- [ ] Locations scouted
- [ ] Visual elements planned
- [ ] Cliffhanger approved
- [ ] CTA for next episode

### 7. Quick Reference
- Key dialogue snippets
- Important visual beats
- Emotional peaks

## Styling Requirements
- Clean, professional design
- Use CSS Grid or Flexbox for layout
- Mobile-responsive
- Print-friendly
- Consistent color scheme
- Readable typography

## Technical Requirements
- Single self-contained HTML file (no external dependencies)
- All CSS inline or in <style> block
- No JavaScript required (or inline only)
- Valid HTML5
- Works offline

Save to: outputs/writers_room_dashboard.html

Before generating, describe your layout approach and color scheme.
```

---

## Quick Prompt Reference

| #   | Prompt              | Output                      | Time  |
| --- | ------------------- | --------------------------- | ----- |
| 1   | Workspace Scan      | workspace_scan.md           | 2 min |
| 2   | Premise Development | premises_options.md         | 3 min |
| 3   | Episode Beats       | episode_beats_001.md        | 3 min |
| 4   | Script Draft        | episode_001.md              | 5 min |
| 5   | HTML Dashboard      | writers_room_dashboard.html | 3 min |

**Total estimated time**: 15-20 minutes
