# Edge cases

Fences with unusual info strings, spacing and line endings.

```
no language at all
```

```unknown-language
unknown languages yield no runs
```

```   rust   
fn padded_info() {}
```

```rust,ignore
fn with_attributes() {}
```

``` {.python .numberLines}
print("pandoc style")
```

````swift
let fourBackticks = "```"
```
still inside the four-backtick fence
````

   ```python
indented_fence = True
   ```

```SQL
SELECT 1;
```

```Python3
print("alias with capitals")
```

```js
const unclosedFence = true;
