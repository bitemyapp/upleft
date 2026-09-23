# JSON, YAML, TOML, CSS

```json
{
  "name": "upleft",
  "version": "0.1.0",
  "private": true,
  "count": -12.5e+3,
  "nothing": null,
  "list": [1, 2, "three", false],
  "nested": { "key" : "value", "escaped \"key\"": "x" }
}
```

```jsonc
{
  // comments are allowed in JSONC
  "editor.fontSize": 14, /* inline */
  "trailing": [1, 2,],
}
```

```yaml
# YAML comment
name: upleft
version: 0.1.0
on:
  push:
    branches: [main, "release/*"]
enabled: yes
disabled: Off
empty: ~
nothing: null
key-with-dashes: value
dotted.key: 'single quoted with \ backslash'
url: http://example.com:8080/path
list:
  - item one
  - "item two": nested
number: 0x1F
time: 12:30:00
```

```yml
a: b # comment
```

```toml
# TOML comment
title = "TOML example"
[package]
name = "upleft-render"
version = "0.1.0"
authors = ["Chris <cma@example.com>"]
edition = '2024'
[dependencies.objc2]
version = "0.6"
features = ["std"]
[[bin]]
name = "conform"
multi = """
multi-line basic
"""
literal = '''
multi-line literal \n
'''
enabled = true
pi = 3.14
date = 1979-05-27T07:32:00Z
key-with-dash = 1
  [indented.table]
```

```css
@import url("theme.css");
@media (prefers-color-scheme: dark) and (min-width: 640px) {
  :root { --main-color: #1e1e1e; --spacing: -5px; }
}
/* comment */
body > .container:hover::after, #main a[href^="http"] {
  color: var(--main-color) !important;
  margin: 0 auto -5px 1.5em;
  background: url('image.png') no-repeat, linear-gradient(to right, #fff 0%, transparent 100%);
  font-family: "SF Mono", Menlo, monospace;
  width: calc(100% - 2rem);
  transition: opacity 0.2s ease-in-out;
  color: currentColor; display: none; content: inherit;
}
```

```scss
$primary: #333; .a { &:hover { color: darken($primary, 10%); } }
```
