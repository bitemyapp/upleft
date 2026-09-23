# JavaScript, TypeScript, JSX, TSX

```javascript
// line comment
/* block
   comment */
import { readFile } from 'node:fs/promises';
export default async function load(path, { encoding = "utf8" } = {}) {
  const MAX_SIZE = 1024 * 1024;
  let text = await readFile(path, encoding);
  const template = `Hello ${name}, you have ${count + 1} messages
  and a second line`;
  if (typeof text !== 'string' || text.length > MAX_SIZE) throw new Error("too big");
  return JSON.parse(text) ?? null ?? undefined ?? NaN ?? Infinity;
}
class Widget extends Base { static #count = 0n; get size() { return this.#count; } }
@decorator class Decorated {}
const re = /ab+c/gi, n = 0xFF + 0b101 + 0o17 + 1_000 + .5e+10;
const s = "unterminated
```

```js
const x = [1, 2, 3].map((v) => v * 2).filter(Boolean);
```

```typescript
interface Shape { readonly kind: "circle" | 'square'; area(): number }
type Maybe<T> = T | null | undefined;
enum Color { Red = 1, Green, Blue }
abstract class Base<T extends object = {}> implements Shape {
  declare readonly kind: "circle";
  private static instances: Map<string, Base<any>> = new Map();
  protected constructor(public readonly id: string) {}
  abstract area(): number;
}
function assertIsString(value: unknown): asserts value is string {}
let v = obj satisfies Record<keyof typeof obj, boolean>;
namespace NS { export const PI = 3.14; }
```

```ts
let u: bigint = 10n; let s: symbol = Symbol("s"); let never_: never;
```

```jsx
export const App = ({ items }) => (
  <ul className="list">
    {items.map((item) => <li key={item.id}>{item.label}</li>)}
  </ul>
);
```

```tsx
const Button: React.FC<Props> = ({ onClick, children }: Props) => <button onClick={onClick}>{children}</button>;
```
