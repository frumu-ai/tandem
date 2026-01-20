# Web Starter Project - Known Issues & Improvements

This document describes known issues and potential improvements for the web starter project. Use this as a reference during the audit process.

## Priority 1: Critical Accessibility Issues

### TODO-001: Missing Form Labels

**Issue**: The search form lacks a proper `<label>` element. The input has `aria-label` but no associated label element.

**Current Code**:

```html
<input type="text" placeholder="Search..." aria-label="Search" class="search-input" />
```

**Expected Fix**: Add proper `<label>` with `for` attribute matching input `id`

**Impact**: Users with screen readers cannot properly identify this form field

**Severity**: Critical

---

### TODO-002: Heading Hierarchy

**Issue**: Page uses `<h1>` followed by `<h4>`, skipping `<h2>` and `<h3>`. This breaks screen reader navigation.

**Current Structure**:

```html
<h1>Welcome to Our Site</h1>
...
<h4>Featured Categories</h4>
```

**Expected Fix**: Use proper heading hierarchy (h1, h2, h3, h4 in order)

**Impact**: Screen reader users cannot navigate content hierarchically

**Severity**: Critical

---

### TODO-003: Missing Button Label

**Issue**: The submit button has no accessible name. Screen readers will announce it as "button" with no context.

**Current Code**:

```html
<button class="btn-primary">Submit</button>
```

**Expected Fix**: Add `aria-label` or use visually hidden label text

**Impact**: Button purpose is unclear to assistive technology users

**Severity**: Major

---

## Priority 2: Major Issues

### TODO-004: Image Alt Text

**Issue**: The logo image has `alt=""` which is empty. While sometimes intentional for decorative images, this is a functional logo that should identify the site.

**Current Code**:

```html
<img src="logo.svg" alt="" class="logo" />
```

**Expected Fix**: Provide descriptive alt text like "Acme Corp Homepage"

**Impact**: Users don't know what site they're on

**Severity**: Major

---

### TODO-005: Navigation Keyboard Access

**Issue**: The main navigation doesn't have proper focus styles and may not be keyboard accessible.

**Expected Fix**: Add visible focus styles, ensure tab order is logical

**Impact**: Keyboard-only users cannot navigate effectively

**Severity**: Major

---

### TODO-006: Color Contrast

**Issue**: Some text colors have insufficient contrast against backgrounds.

**Affected Elements**:

- Footer text: #999999 on #f5f5f5 (fails WCAG AA)
- Secondary button text: #666666 on white (fails WCAG AA)

**Expected Fix**: Use colors with 4.5:1 contrast ratio minimum

**Impact**: Users with low vision cannot read content

**Severity**: Major

---

## Priority 3: Minor Issues

### TODO-007: Form Placeholder as Label

**Issue**: Using placeholder text as a visual label violates WCAG guidelines.

**Expected Fix**: Add visible label elements, use placeholder for hints only

**Impact**: Reduces usability, especially when focus is lost

**Severity**: Minor

---

### TODO-008: Link Ambiguity

**Issue**: Multiple links use "Learn More" as text, making them indistinguishable.

**Expected Fix**: Use descriptive link text or add `aria-label` for context

**Impact**: Users can't understand where links will take them

**Severity**: Minor

---

### TODO-009: JavaScript Sorting Bug

**Issue**: The product sorting in app.js has a logic error. The sort function doesn't properly handle numeric sorting, causing "10" products to appear before "2" products.

**Expected Fix**: Use numeric comparison in sort function

**Impact**: Products are displayed in wrong order, confusing users

**Severity**: Major

---

### TODO-010: Repetitive Code Pattern

**Issue**: CSS has significant repetition in button styles across multiple classes.

**Example**:

```css
.btn-primary {
  background: blue;
  padding: 10px 20px;
  border-radius: 4px;
}
.btn-secondary {
  background: blue;
  padding: 10px 20px;
  border-radius: 4px;
}
.btn-success {
  background: blue;
  padding: 10px 20px;
  border-radius: 4px;
}
```

**Expected Fix**: Extract shared styles to a base button class

**Impact**: Harder to maintain, larger file size

**Severity**: Minor

---

## Priority 4: Suggestions

### TODO-011: ARIA Live Regions

**Issue**: Dynamic content updates (filtering, sorting) don't announce to screen readers.

**Expected Fix**: Add `aria-live` regions for dynamic content changes

**Impact**: Screen users miss important updates

**Severity**: Suggestion

---

### TODO-012: Skip Navigation Link

**Issue**: No skip-to-content link for keyboard users.

**Expected Fix**: Add hidden skip link as first focusable element

**Impact**: Keyboard users must tab through all navigation

**Severity**: Suggestion

---

### TODO-013: Focus Management

**Issue**: After filtering/sorting products, focus is not managed, leaving users disoriented.

**Expected Fix**: Manage focus programmatically after content changes

**Impact**: Users lose context after interactions

**Severity**: Suggestion

---

## Testing Checklist

- [ ] Run Lighthouse accessibility audit
- [ ] Test with keyboard only navigation
- [ ] Test with screen reader (NVDA, VoiceOver, or similar)
- [ ] Verify color contrast with contrast checker
- [ ] Test form with validation errors
- [ ] Test sorting and filtering with assistive technology

---

## Reference Standards

- WCAG 2.1 Level AA compliance target
- WAI-ARIA Authoring Practices 1.1
- HTML5 semantic elements usage
