---
title: Guide
status: draft
summary: "How the sample fits together"
---

# Guide

Read this after the [README](../README.md). The [API reference](api/reference.md)
lists every endpoint; [[README]] is the entry point.

## Setup

Install the tools. Then ship the release.

```sh
make install   # the guide's code block
```

## Usage

| Command | Effect |
|---|---|
| `make` | builds the [tutorial](tutorial.mdx) |
| `make docs` | writes [chapter 10](numbered/chapter%2010.md) |

> [!NOTE]
> The guide links to the [missing page](missing.md) on purpose.

![photo](../assets/photo.jpg)
![local](../export/local.png)

### Deeper

A path token `src/main.rs:12` and a link back [up](../notes.md#setup).
