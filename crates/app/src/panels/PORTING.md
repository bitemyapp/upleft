# Panels: porting notes

The port of `Sources/DownrightApp/Panels/` (37 Swift files) to
`crates/app/src/panels/`. The binding rules are the repository's `AGENTS.md`
("App identity", "Window captures are headless") and `crates/app/PORTING.md`.

## File map

One Rust module per Swift file, snake_case (`TaskPanelView.swift` →
`task_panel_view.rs`); `mod.rs` declares all of them. `appkit_support.rs` is
the only Rust-only module: the Swift overlay conveniences the panels use
(`activate([...])`, `null_actions`, `without_actions`, role statics,
`set_label`/`set_role`/`set_value`/`set_help` on any object, `downcast`,
`superview`, `set_mask`, `Presentation`, `cg`, `cg_array`, symbol images).

`app/toolbar_controls.rs` is a **partial** port of `App/ToolbarControls.swift`
(`ToolbarChromePolicy`, `ToolbarInteractiveButton`), made here because the
panels need it; `port/app-shell` owns that file.

## Conventions (follow them in every panel)

- **Class names.** Every Swift class, `private` ones included, is a
  `define_class!` type whose Objective-C name is the Swift unqualified name
  (`#[name = "HealthDiagnosticRowView"]`). The `panel` dump compares class
  names.
- **Style sheets** are `Rc<StyleSheet>`. A Swift `var styleSheet: StyleSheet
  { didSet { … } }` is `style_sheet()` plus `set_style_sheet(Rc<StyleSheet>)`
  running the `didSet` body. `StyleSheet.current` is
  `Rc::new(StyleSheet::current(mtm))`.
- **Properties with observers** are `x()` / `set_x(…)` running the observer.
  Swift's `didSet` does not run during `init`; the Rust constructor sets the
  ivar directly there too.
- **Ivars** are `Cell`/`RefCell`, borrowed only for the statement that uses
  them (AppKit re-enters). Never hold a `borrow()` across a call into AppKit
  or a callback: clone the `Rc`/`Retained` out first.
- **Initialisers.** `Self::alloc(mtm).set_ivars(…)` then
  `msg_send![super(this), initWithFrame: …]`, then the rest of the Swift
  `init` body in the same order. Swift evaluates stored-property initial
  values before `super.init`; create those objects before `set_ivars`.
  A class AppKit may instantiate itself (`+buttonWithImage:…` on a
  subclass, `makeViewWithIdentifier` fallbacks, `NSTableRowView`) overrides
  `initWithFrame:` to set its ivars (see `PanelSymbolButton`).
  A Swift base class another class subclasses (`MessageBarView`) takes its
  Rust-typed arguments through a staged thread-local (`MessageBarInit`) read
  in its `initWithFrame:`.
- **Overridable methods** (a Swift `class` method a subclass overrides) are
  Objective-C methods; the base calls them with `msg_send![self, …]`, the
  override calls `msg_send![super(self), …]` (`MessageBarView.applyStyle`,
  `ToolbarInteractiveButton.permitsHoverFeedback`).
- **Overrides with early returns.** Inside `define_class!`, a method
  returning `bool` or `Option<Retained<…>>` must not use `return` or `?` in
  its body (the macro wraps the body); delegate to an inherent helper
  (`fn hit_test(&self, …)`), as `PanelCheckbox.hitTest:` does.
- **Delegates** are Rust traits held as `Weak<dyn Trait>` (`std::rc::Weak`),
  as `MarkdownTextViewDelegate` is in the render crate. Protocol-extension
  defaults are trait default methods.
- **Closures** (`var onClose: (() -> Void)?`) are
  `RefCell<Option<Rc<dyn Fn(…)>>>` with `set_on_close(Option<Rc<…>>)`; clone
  the `Rc` out before calling it.
- **`ButtonAction`** is the closure target (`ButtonAction::new(|| …, mtm)`,
  selector `ButtonAction::selector()`); the panel keeps it alive in an ivar
  exactly where Swift keeps it.
- **Swift casts to panel types** that live in another module are Objective-C
  class checks (`appkit_support::is_kind_of(view, c"TaskPanelView")`) plus a
  selector. The selectors other modules call:
  - `preferredWidth` → `CGFloat` on every `PanelSurface` panel
    (`FloatingPanelSurface.preferred_width` reads it);
  - `TaskPanelView`: `focusForPresentation`, `fittedContentHeight` → `CGFloat`;
  - `FrontMatterEditorView`: `focusField`.
- **Numbers.** Swift `min`/`max` on `CGFloat` are `smin`/`smax` (NaN order);
  `rounded()` is `f64::round`; `Int(x)` truncates; keep the evaluation order.
  `CGRect.width`/`minX`… are the `RectExt` methods (standardised).
- **Strings.** Swift `String` counts and comparisons go through
  `upleft-swift-text`; `NSString` bridging through `ns_string`.
- **Main thread.** Nothing here may block it. Where Swift does I/O on the
  main thread in a panel, do the same thing (fidelity) and list it under
  "Main-thread I/O" below.
- **Test hooks.** Swift `…ForTesting` members are `pub fn …_for_testing`.

## Conformance

Two suites, both `"oracle": "app"`:

- `panel` (`corpus/panels/*.json`, PNG + layout dump): one panel built from
  a scenario, hosted in a borderless window at (-30000, -30000) (titled
  windows too, with `constrainFrameRect:toScreen:` made the identity by the
  app-window harness's `OffScreenWindows`, and a guard that exits if any
  window touches a display), never activated, settled on three identical
  `cacheDisplay` captures, then captured by the window server
  (`CGWindowListCreateImage`, via `WindowServerCapture`), glass and
  materials included. The layout dump is the full view tree (`PanelTree`).
- `panel-model` (`corpus/panel-models/*.json`, JSON): the same panels built
  windowless for a list of states, laid out at the scenario size, dumped.

Scenario envelope: `{"panel": "<Swift type>", "theme": "Paper Light",
"dark": false, "width": 336, "height": 480, "document":
"corpus/generated/agent/agent-400.md", "state": {…}}`; `panel-model` adds
`"states": [{"name": "…", …}]`, each merged over `state`.

Scenes: `oracle/app/Sources/downright-app-oracle/Panels/Scenes/<Type>Scene.swift`
(Swift, `@testable import DownrightApp`) and
`crates/conformance/src/dump/panel/scenes/<type>.rs` (Rust). Each builds the
panel, applies `state` with the same calls in the same order, and returns a
`model()` of panel-specific derived values (counts, captions, geometry,
test hooks). The harness style sheet forces Reduce Motion on (as the render
harness does); scenes note what their panel does with it.

Run: `just app-oracle` (or `swift build --package-path oracle/app -c release
-Xswiftc -enable-testing --scratch-path target/app-oracle` then `just stamp
target/app-oracle/release/downright-app-oracle`), `cargo build --release -p
upleft-conformance -p upleft-cli`, then `target/release/conform --suite panel
--suite panel-model [--filter NAME]`. `scripts/jsondiff.py swift.json
rust.json` localises a dump difference.

## Status

See the table at the end of this file (kept current by each commit).
