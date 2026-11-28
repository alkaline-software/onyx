The new site goes live in December. Plain pages, one stylesheet, no build step.

## This week

- [x] Move the journal to its own folder
- [x] Dark appearance
- [ ] Write the About page
- [ ] Check every page on a phone

## The feed

A small script makes the feed from the posts' folder:

```python
from pathlib import Path

def posts(folder):
    pages = Path(folder).glob("*.md")
    for page in sorted(pages):
        text = page.read_text()
        yield page.stem, text[:40]

for day, words in posts("journal"):
    print(day, words)
```

## Colors

| Name | Light | Dark |
|---|---|---|
| Ground | `#FAF9F7` | `#1C1B1A` |
| Ink | `#1F1E1D` | `#ECEAE6` |
| Accent | `#2B7A8C` | `#6DB8C8` |

Notes from the call with Emma are in [[Meeting notes]].
