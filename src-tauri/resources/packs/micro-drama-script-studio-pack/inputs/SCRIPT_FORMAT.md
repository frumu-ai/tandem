# Script Format Guide

## Standard Micro-Drama Format

### Episode Header

```
Episode: [Number]
Title: [Episode Title]
Runtime: [MM:SS]
Genre: [Genre]
Characters: [List]
```

### Scene Headings

Use standard screenplay format:

```
INT. [LOCATION] - [TIME]
```

Or for exterior shots:

```
EXT. [LOCATION] - [TIME]
```

Common locations: COFFEE SHOP, APARTMENT, OFFICE, STREET, PARK, ROOFTOP, GYM

Common times: DAY, NIGHT, MORNING, EVENING, DAWN, DUSK

### Action Lines

Write in present tense. Describe only what can be seen or heard.

**Example:**

```
A rushes into the coffee shop. Scans the room. Spots B in the corner.
```

**Rules:**

- Maximum 2-3 lines per action block
- One thought per sentence
- Avoid internal monologue (show through action)

### Character Names

### Character Names

Use full names or descriptive roles for clarity.

**Option 1: Full Names (Preferred)**

```
SARAH:
Dialogue goes here.
```

**Option 2: Descriptive Roles**

```
MOTHER:
Dialogue here.

BOSS:
Get back to work.
```

Avoid single letters (A, B) unless the characters are intentionally anonymous.

### Dialogue

```
CHARACTER:
Dialogue line.
```

Keep dialogue under 20 words when possible.

### Parentheticals

Use sparingly for:

- Physical action affecting delivery
- Who they're speaking to
- Important emotional beat

```
A:
(whispering)
I can't believe you came.
```

### Pauses

Use [beat] for significant pauses:

```
[beat]
A realizes the truth.
```

### Scene Transitions

```
FADE TO:

or

SMASH CUT TO BLACK.

or

TRANSITION:
```

### Cliffhanger Convention

End episodes with:

```
[beat]

CHARACTER:
Final line that creates intrigue.

[END EPISODE - CLIFFHANGER]
```

Or use:

```
TEXT ON SCREEN: "[Message]"

[END EPISODE]
```

### Dialogue-Only Scenes

For minimal action, dialogue-heavy scenes:

```
INT. COFFEE SHOP - DAY

A:
I never thought I'd see you here.

B:
Life's full of surprises.

A:
(softly)
Too many, sometimes.
```

---

## Complete Example

```
Episode: 001
Title: The Wrong Number
Runtime: 02:00
Genre: Romance
Characters: A, B

---

INT. COFFEE SHOP - MORNING

A sits alone, typing on laptop. Phone buzzes.

A:
(grabbing phone)
Another text from Mom.

Ignores it. Types more.

---

INT. APARTMENT - NIGHT

A collapses on couch. Phone buzzes. Different number.

B: (TEXT)
Wrong number. But since you're reading—hope your day got better.

A stares at screen. Types reply.

A: (TEXT)
"It didn't. But this made me smile."

---

INT. COFFEE SHOP - ONE WEEK LATER

A at same spot. Phone buzzes.

B: (TEXT)
"I keep texting this number. You still there?"

A looks around shop.

A: (TEXT)
"I'm here. Who are you?"

B: (TEXT)
"Would you like to find out?"

[beat]

A types. Hesitates. Types again.

A: (TEXT)
"Meet me here tomorrow. 10 AM."

---

INT. COFFEE SHOP - NEXT MORNING

A at corner table. Checking phone. Checking door.

DOOR CHIME

A looks up—

SMASH CUT TO BLACK.

TEXT ON SCREEN: "TO BE CONTINUED..."

[END EPISODE - CLIFFHANGER]
```

## LLM Formatting Notes

When generating scripts via LLM, use standard Markdown formatting to ensure the scripts render correctly in the Tandem app.

### Guidelines for LLM

1.  **Markdown Compatible**: Ensure all output is valid Markdown.
2.  **Visual Hierarchy**: Use bolding and headers to distinguish between scene headings, characters, and action lines.
3.  **Spacing & Line Breaks**:
    - **Left-Aligned**: This micro-drama format is strictly left-aligned. Do not attempt to center character names or dialogue using spaces, as this breaks on different screen sizes.
    - **Vertical Separation**: Use a full empty line between distinct elements (e.g., between Action and Character, or between Dialogue and next Character) to ensure clear paragraph rendering.
    - **Tight Dialogue**: To keep the Character Name and Dialogue visually connected, you may use a single line break if the renderer supports it, but standard double-newline paragraphs are safest for compatibility.
4.  **Character Naming**:
    - **Random Generation**: If character names are not provided, automatically generate suitable, random names based on the character's gender and role (e.g., **SARAH**, **MIKE**).
    - **Avoid Placeholders**: Do not use generic "A", "B", or "C" unless explicitly requested.
5.  **Code Blocks**: You may wrap the script in a markdown code block for easy copying, but plain markdown is preferred for direct rendering.

### Prompting Information

To get the best results, instruct the LLM with:

> "Format the script in Markdown. Use bold for Scene Headings (e.g., **INT. ROOM - DAY**) and Character Names (e.g., **JOHN**). Italicize parentheticals. Keep dialogue as plain text. If names are not provided, generate random, suitable names for all characters."

### Example Markdown Render

**INT. SPACESHIP COCKPIT - NIGHT**

Stars streak past the viewport. **CAPTAIN** grips the controls.

**CAPTAIN**
_(shouting)_
Hold on!

**EXT. SPACE - CONTINUOUS**

The ship barrel-rolls through an asteroid field.
