# HTML, XML, diff, Markdown, plain text

```html
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>Example &amp; test &#169; &#x1F600; & alone &;</title>
  <!-- a comment -->
  <script src="app.js" defer></script>
</head>
<body class='main' data-x=unquoted hidden>
  <p>Text with <em>emphasis</em> and <a href="https://example.com/?a=1&b=2">a link</a>.</p>
  <br/><img src="a.png" alt="" />
  <input type="checkbox" checked>
</body>
</html>
```

```xml
<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
  <key>CFBundleName</key>
  <string>Upleft</string>
  <data><![CDATA[ raw <data> & stuff ]]></data>
  <ns:element xmlns:ns="urn:x" ns:attr='v'/>
</dict>
</plist>
```

```svg
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><circle cx="5" cy="5" r="4"/></svg>
```

```htm
<p>unterminated <!-- comment
```

```diff
diff --git a/src/lib.rs b/src/lib.rs
index 83db48f..bf269f4 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,5 +1,6 @@
 use std::fmt;
-use std::io;
+use std::io::{self, Write};
+use std::path::Path;
 
 fn main() {
\ No newline at end of file
new file mode 100644
deleted file mode 100644
old mode 100644
new mode 100755
similarity index 90%
rename from a.txt
rename to b.txt
Binary files a/x.png and b/x.png differ
```

```patch
+added
-removed
 context
```

~~~~markdown
# Heading one
## Heading two ##
####### seven hashes is text
#not a heading

Paragraph with **bold**, *italic*, ~~strike~~, `code`, ``double `tick` code``, and [a link](https://example.com "title").
![image alt](path/to/image.png) and [ref link][ref] and [unclosed link
> A quote with *emphasis*
> > nested quote
- item one
* item two
+ item three
1. first
2) second
10. tenth
-not a list
---
***
_ _ _
```swift
let fenced = "inside markdown"
```
~~~
tilde fence
~~~
[ref]: https://example.com
~~~~

```md
Unclosed `code span and [link](with (parens) destination
```

```plaintext
Plain text is known but never coloured: let x = 1; // not a comment
```

```text
also plain
```
