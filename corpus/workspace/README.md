---
title: Sample Workspace
tags: [workspace, links]
owner: docs-team
---

# Sample Workspace

This workspace exercises the index, the link graph and search. Start with the
[guide](docs/guide.md), then read the [notes](./notes.md) and [[Notes]].

## Links that resolve

- A labelled wikilink: [[deep/note|the deep note]].
- A wikilink by stem: [[guide]] and by path [[docs/api/reference]].
- A link with a fragment: [guide, setup](docs/guide.md#setup).
- A link with a query: [guide?](docs/guide.md?mode=read#setup).
- An encoded name: [plan](my%20plan.md) and an angle-bracket one: [plan too](<my plan.md>).
- A root-anchored link: [api](/docs/api/reference.md).
- Without an extension: [tutorial](docs/tutorial) and [chapter 2](docs/numbered/chapter%202).
- Through dot segments: [reference](./docs/../docs/api/reference.md).
- A Unicode file: [café](unicode/café.md), [[unicode/日本語]], [party](<unicode/emoji 🎉.md>).

## Links that do not

- Missing: [missing](missing.md), [[Nowhere]], [gone](docs/gone.md#x).
- Wrong case: [CASE](CASENOTES.md) and [[casenotes]].
- Ambiguous stem: [[leaf]] and [[deep/nested/level3/leaf]].
- Bad escape: [bad](bad%zz.md) and a lone byte [latin](caf%E9.md).
- External: [site](https://example.com/docs/guide.md), <https://example.org>,
  [mail](mailto:team@example.com), [js](javascript:alert(1)), [data](data:text/plain,hi),
  [file](file:///etc/hosts), [anchor](#links-that-resolve).
- Empty wikilink target: [[ ]] and [[#Links that do not]].

![diagram](assets/diagram.png)
![inside link](assets/inside.png)
![escape](assets/escape.png)
![photo](assets/photo.jpg "A photo")
![icon](assets/icon.svg)
![upper](assets/Upper.PNG)
![empty](assets/empty.png)
![unknown](assets/blob.bin)
![csv](assets/table.csv)
![missing](assets/none.png)
![remote](https://example.com/pixel.png)
![absolute](/etc/hosts)
![parent](../README.md)

The guide mentions the plan. Notes about the guide live elsewhere.
