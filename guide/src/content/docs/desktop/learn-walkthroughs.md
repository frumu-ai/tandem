---
title: Desktop Learn Walkthroughs
---

You can keep your visual walkthroughs in Learn and embed them directly into docs pages.

## Learn Hub

- External hub: [tandem.frumu.ai/learn](https://tandem.frumu.ai/learn)

## Embed Pattern

Use this HTML block in any docs page to embed a walkthrough:

```html
<div
  style="position:relative;padding-top:56.25%;border:1px solid var(--sl-color-gray-5);border-radius:12px;overflow:hidden;"
>
  <iframe
    src="https://tandem.frumu.ai/learn"
    title="Tandem Learn Walkthrough"
    loading="lazy"
    style="position:absolute;inset:0;width:100%;height:100%;border:0;"
    referrerpolicy="strict-origin-when-cross-origin"
    allowfullscreen
  ></iframe>
</div>
```

## Notes

- Prefer embedding specific walkthrough URLs when available, not only the Learn index.
- Pair each embed with a short text checklist so users can follow along without video/audio.
