# Python

```python
#!/usr/bin/env python3
"""Module docstring with 'quotes' and "doubles"."""
from __future__ import annotations
import os, sys

MAX_RETRIES = 3
_private = None

@dataclass(frozen=True)
class Config(Base):
    '''Class docstring
    over lines'''
    name: str = "default"
    size: int = 0x10 + 0o7 + 0b11 + 1_000 + 1.5e-3 + .25j

    def __init__(self, *args, **kwargs) -> None:
        self.path = r"C:\raw\string"
        self.fmt = f"{self.name!r:>10}"
        self.b = b'bytes' + B"BYTES" + rb"raw\bytes" + Rb'x' + BR"y" + rf"{x}\n" + Fr'z'
        self.u = u"unicode é"
        self.bad = "unterminated
        if not (a and b or c) is None: pass
        lambda x: x ** 2 // 3 % 4 @ m
        print(True, False, None, NotImplemented, Ellipsis, cls)
        return [x for x in range(10) if x in {1, 2}]

async def main():
    await asyncio.sleep(1)  # trailing comment
    match command:
        case "go": yield from gen()
```

```py
x = 'single' 'adjacent'
y = """unterminated triple
```
