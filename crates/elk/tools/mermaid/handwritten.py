#!/usr/bin/env python3
"""Hand-written Mermaid diagrams for the ELK corpus: every direction, node
shape, edge style, labels, subgraphs (nested, direction overrides, edges
across), self-loops, multi-edges, chains, fan-outs, cycles, state, class and
ER diagrams. Prints a JSON list of {name, source}."""
import json

d = []
def add(name, src):
    d.append({"name": "hw-" + name, "source": src.strip("\n")})

shapes = "\n".join([
    "  a[Rectangle] --> b(Rounded)", "  b --> c([Stadium])", "  c --> d[[Subroutine]]", "  d --> e[(Database)]",
    "  e --> f((Circle))", "  f --> g(((Double)))", "  g --> h>Asymmetric]", "  h --> i{Diamond}",
    "  i --> j{{Hexagon}}", "  j --> k[/Parallelogram/]", "  k --> l[\\Alt parallel\\]", "  l --> m[/Trapezoid\\]",
    "  m --> n[\\Trapezoid alt/]"])
for dirn in ["TD", "TB", "BT", "LR", "RL"]:
    add(f"shapes-{dirn}", f"flowchart {dirn}\n{shapes}")
    add(f"labels-{dirn}", f"""graph {dirn}
  A[Start] -->|yes| B[Continue]
  A -->|no| C[Stop]
  B -- long label text here --> D[Next step]
  C -. dotted label .-> D
  D == thick ==> E[End]
  E <--> A
  B --- F[Undirected]""")
    add(f"cycle-{dirn}", f"""graph {dirn}
  A --> B --> C --> D --> A
  C --> E --> B
  E --> F
  F --> A""")
    add(f"selfloops-{dirn}", f"""flowchart {dirn}
  A --> A
  A --> B
  B -->|again| B
  B --> C
  C --> C
  C -->|loop| C
  C --> A""")
    add(f"subgraphs-{dirn}", f"""flowchart {dirn}
  X[Outside] --> A
  subgraph one [First group]
    A[Alpha] --> B[Beta]
  end
  subgraph two [Second group]
    C[Gamma] --> D[Delta]
    subgraph inner [Inner]
      E[Epsilon]
    end
    D --> E
  end
  B --> C
  E --> Y[Exit]""")
    add(f"subgraph-dir-{dirn}", f"""flowchart {dirn}
  start --> A1
  subgraph left
    direction LR
    A1 --> A2 --> A3
  end
  subgraph right
    direction TB
    B1 --> B2
    B1 --> B3
  end
  A3 --> B1
  B2 --> done
  B3 -->|label| done""")

add("multi-edges", """graph LR
  A --> B
  A --> B
  A -->|one| B
  B --> A
  B -->|back| A
  A --> C
  C --> A""")
add("chain-30", "graph TD\n" + "\n".join(f"  n{i} --> n{i+1}" for i in range(30)))
add("chain-60-lr", "graph LR\n" + "\n".join(f"  n{i}[Step {i}] --> n{i+1}[Step {i+1}]" for i in range(60)))
add("fanout-20", "graph TD\n" + "\n".join(f"  root --> c{i}[Child {i}]" for i in range(20)))
add("fanin-20", "graph LR\n" + "\n".join(f"  s{i} --> sink((Sink))" for i in range(20)))
add("fanout-fanin", "graph TD\n" + "\n".join(f"  top --> m{i}\n  m{i} --> bottom" for i in range(12)))
add("high-degree", "graph TD\n" + "\n".join(f"  hub --> leaf{i}" for i in range(18)) + "\n" + "\n".join(f"  src{i} --> hub" for i in range(17)) + "\n  leaf3 --> deep1 --> deep2")
add("high-degree-trees", "graph LR\n" + "\n".join(f"  hub --> t{i}\n  t{i} --> t{i}a\n  t{i} --> t{i}b" for i in range(17)) + "\n" + "\n".join(f"  p{i}a --> p{i}\n  p{i} --> hub" for i in range(17)))
add("disconnected", """graph TD
  A --> B
  C --> D
  E
  F --> G --> H
  I((lonely))
  J --> J""")
add("grid", "graph LR\n" + "\n".join(f"  g{r}_{c} --> g{r}_{c+1}" for r in range(5) for c in range(5)) + "\n" + "\n".join(f"  g{r}_{c} --> g{r+1}_{c}" for r in range(4) for c in range(6)))
add("complete-k6", "graph TD\n" + "\n".join(f"  k{a} --> k{b}" for a in range(6) for b in range(6) if a != b))
add("dag-diamonds", "graph TD\n" + "\n".join(f"  d{i} --> d{i}l\n  d{i} --> d{i}r\n  d{i}l --> d{i+1}\n  d{i}r --> d{i+1}" for i in range(8)))
add("multiline-labels", """graph TD
  A["First line<br/>second line"] -->|"edge line one<br/>edge line two"| B["Another<br/>multi<br/>line node"]
  B --> C{"Decision<br/>point"}
  C -->|a| D
  C -->|b| E["Wide label with many many many many words in it"]""")
add("unicode", """graph LR
  A[Café ☕] --> B[日本語テキスト]
  B --> C[Emoji 🚀 launch]
  C -->|naïve| D[Ωmega]""")
add("nested-deep", """flowchart TD
  subgraph L1 [Level 1]
    subgraph L2 [Level 2]
      subgraph L3 [Level 3]
        a --> b
      end
      c --> a
    end
    d --> c
  end
  e --> d
  b --> f""")
add("subgraph-edges", """flowchart LR
  subgraph S1
    a1 --> a2
  end
  subgraph S2
    b1 --> b2
  end
  subgraph S3
    c1
  end
  S1 --> S2
  a2 --> c1
  c1 --> b1
  x --> S3""")
add("subgraph-dir-nested", """flowchart TB
  subgraph outer
    direction LR
    subgraph inner
      direction TB
      i1 --> i2
    end
    o1 --> i1
  end
  i2 --> z
  y --> o1""")
add("subgraph-cross", """graph TD
  subgraph A
    a1 --> a2
    a2 --> a3
  end
  subgraph B
    b1 --> b2
  end
  a1 --> b1
  b2 --> a3
  a3 --> c
  c --> b1""")
add("state-simple", """stateDiagram-v2
  [*] --> Idle
  Idle --> Running : start
  Running --> Paused : pause
  Paused --> Running : resume
  Running --> Idle : stop
  Running --> [*]""")
add("state-composite", """stateDiagram-v2
  [*] --> First
  state First {
    [*] --> second
    second --> third
    third --> [*]
  }
  First --> Last
  Last --> Last : retry
  Last --> [*]""")
add("state-lr", """stateDiagram-v2
  direction LR
  [*] --> A
  A --> B : go
  B --> C
  C --> A : back
  C --> [*]""")
add("class-relations", """classDiagram
  class Animal {
    +String name
    +int age
    +makeSound() void
  }
  class Dog {
    +fetch() void
  }
  class Cat
  class Owner {
    -List~Animal~ pets
  }
  Animal <|-- Dog
  Animal <|-- Cat
  Owner "1" o-- "*" Animal : owns
  Dog ..> Bone : chews
  Owner *-- Address
  Cat ..|> Pet
  Owner --> Vet : visits""")
add("class-wide", "classDiagram\n" + "\n".join(f"  Base <|-- Derived{i}" for i in range(10)) + "\n" + "\n".join(f"  Derived{i} --> Helper{i%3} : uses" for i in range(10)))
add("er-shop", """erDiagram
  CUSTOMER ||--o{ ORDER : places
  ORDER ||--|{ LINE-ITEM : contains
  CUSTOMER }|..|{ DELIVERY-ADDRESS : uses
  PRODUCT ||--o{ LINE-ITEM : "ordered in"
  CUSTOMER {
    string name
    string email PK
  }
  ORDER {
    int id PK
    date created
  }""")
add("er-cycle", """erDiagram
  A ||--o{ B : ab
  B ||--o{ C : bc
  C ||--o{ A : ca
  A ||--|| A : self""")
print(json.dumps(d))
