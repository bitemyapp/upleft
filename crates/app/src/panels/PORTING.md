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

### Harness facts worth knowing

- `PanelFont` reads `Preferences.shared` (Swift) / `Preferences::shared()`
  (Rust), as Downright does. Loading it publishes the Quick Look appearance
  keys (`com.bitemyapp.upleft.quickLook.*`, defaults) to the global
  preferences domain on both sides, as the app-window harness and
  Downright's own tests do; `CFFIXED_USER_HOME` does not isolate that write.
  The runner's sandbox home keeps `preferences.json` at its defaults.
- Class names in the dump are `-class` (Swift: `type(of:)`), so the KVO
  subclass AppKit gives a window once it is ordered in is hidden on both
  sides; AppKit's own Swift classes keep their mangled runtime names.
- Settling uses `cacheDisplay`; the PNG is the window server's composite
  (`CGWindowListCreateImage`), so glass, visual-effect materials and layer
  shadows are in the pixels.
- Reduce Motion: the harness style sheet forces it on. The foundation
  controls then snap (segmented thumb, progress fill, checkbox, floating
  surface reveal, glass focus ring). `PanelSymbolButton` and
  `ToolbarInteractiveButton` default to `StyleSheet.current` (the system
  setting) and only animate on hover and press, which no scene drives.

`just panel-bench` (`scripts/panel-bench-compare.py`) times `bench-panel`
over `corpus/panel-bench/` on both oracles: each state is a panel built and
laid out (and drawn into a bitmap with `"draw": true`), windowless; reading
and parsing the scenario document is cached per process and not timed.

Checks that catch what release builds do not:
- `python3 scripts/check-protocol-selectors.py` verifies every protocol
  method a `define_class!` block declares against the objc2 bindings (a
  misspelt delegate selector is never called in release and panics in
  debug).
- A debug `upleft-oracle` run over `corpus/panel-models/` (`cargo build -p
  upleft-conformance`, then `target/debug/upleft-oracle panel-model …`)
  registers every panel class under objc2's debug checks.

## Status (2026-09-23)

All 37 Swift files are ported. Conformance: `panel` 228/228 and
`panel-model` 39/39 (589 states), 0 fail, 0 error, uncached, against a
clean Swift build. Tests: 113/113 in the eight `panels_*_tests` binaries.

| Swift (`Panels/`) | Rust (`panels/`) | `panel` | `panel-model` states | tests |
|---|---|---:|---:|---|
| PanelChrome | panel_chrome | 19 | 20 | panels_chrome |
| ChromeGlass | chrome_glass | 4 | 8 | panels_chrome |
| FloatingPanelSurface (+ FloatingPanelWindow) | floating_panel_surface | 4 | 6 | — |
| InspectorHostView | inspector_host_view | 6 | 16 | panels_chrome |
| ConflictBarView | conflict_bar_view | 4 | 4 | panels_chrome |
| ActivityIndicatorView | activity_indicator_view | 2 | 4 | — |
| BreadcrumbView | breadcrumb_view | 7 | 19 | panels_toolbar |
| DocumentStatusBarView | document_status_bar_view | 6 | 14 | panels_toolbar |
| TaskProgressRing | task_progress_ring | 8 | 20 | panels_toolbar |
| CommandPaletteView | command_palette_view | 9 | 26 | panels_toolbar |
| FindBarView | find_bar_view | 8 | 22 | panels_find |
| ChangeSummaryBarView | change_summary_bar_view | 6 | 21 | panels_find |
| SearchResultsPanelView | search_results_panel_view | 7 | 19 | panels_find |
| SearchInspectorView | search_inspector_view | 6 | 11 | panels_find |
| TaskPanelView | task_panel_view | 11 | 30 | panels_task |
| TaskSectionBarView | task_section_bar_view | 4 | 13 | panels_task |
| UpdateWindowController | update_window_controller | 12 | 23 | panels_update |
| UpdateNotesPopover (+ UpdateNotesSummary) | update_notes_popover | 6 | 18 | panels_update |
| UpdateStatusPill | update_status_pill | 8 | 19 | panels_update |
| TableEditorView | table_editor_view | 8 | 36 | panels_editor |
| TidySheetView | tidy_sheet_view | 7 | 20 | panels_editor |
| FrontMatterEditorView | front_matter_editor_view | 9 | 39 | panels_editor |
| DocumentHealthView | document_health_view | 6 | 15 | panels_diagnostics |
| RenderTargetsView | render_targets_view | 6 | 17 | panels_diagnostics |
| AssetDoctorView | asset_doctor_view | 5 | 5 | panels_diagnostics |
| DocumentLensView | document_lens_view | 7 | 17 | panels_diagnostics |
| LightboxWindow | lightbox_window | 5 | 17 | panels_diagnostics |
| DocumentQuickLook | document_quick_look | — (draws nothing) | 6 | panels_diagnostics |
| ReviewPanelView | review_panel_view | 5 | 14 | panels_sidebar |
| WorkspaceSidebarView | workspace_sidebar_view | 6 | 19 | panels_sidebar |
| VersionTimelineView | version_timeline_view | 5 | 12 | panels_sidebar |
| HistoryInspectorView | history_inspector_view | 4 | 7 | panels_sidebar |
| TrustPromptView | trust_prompt_view | 5 | 12 | panels_sidebar |
| ReaderProfilePickerView | reader_profile_picker_view | 5 | 17 | panels_sidebar |
| LocalAIPanelView | local_ai_panel_view | 4 | 14 | panels_sidebar |
| VisualDebuggerView | visual_debugger_view | 4 | 9 | panels_sidebar |
| FuzzyMatcher | fuzzy_matcher | (palette suite) | | |

### How the app shell hooks in

- `DocumentQuickLook.swift`'s `extension DocumentWindowController` is the
  `QuickLookHost` trait (`document_quick_look.rs`): the controller implements
  six accessors (`quick_look_owner`, `container_text_view`,
  `markdown_document_url`, `resolve_path_token`, `present_lightbox`,
  `authorize_read_local_asset`) and forwards the `QLPreviewPanel` data-source
  selectors to the trait's provided methods, which carry the Swift bodies.
  The module holds a private copy of `MarkdownLinkDestination.classify` until
  `DocumentWindowController+Delegates.swift` is ported.
- `UpdateWindowController` implements the updater's `UpdatePanelController`
  seam, and every `UpdateCoordinator` is created with that factory, as
  Swift's `showPanel()` builds the controller: `showPanel()` now shows a real
  titled window unless `suppress_ui_for_testing` is set.
- `UpdateStatusPill::new(Presentation::CompactWarning, mtm)` is the start
  window's pill.

### Main-thread I/O (as in Swift)

- `DocumentQuickLook.present_quick_look`: `FileManager.fileExists` and the
  host's `PathResolver.resolve`.
- `ReaderProfilePickerView::new_default`: the JSON profile store in the
  support folder.

### Unverified

Paths no scene or test can drive without a key window or real pointer:
hover washes and press feedback, drag and drop in the task panel, context
menus, the path and zoom pop-up menus, the lightbox's `present(over:)`,
scroll, magnify and drag, the update notes popover's pointer poll and link
click, and every animated branch with Reduce Motion off (the harness forces
it on; a few task-panel animations are checked by tests for consistency, not
pixels).

### Left

Nothing in `Panels/`. The `DocumentWindowController`-driven cases of
WindowChromeTests, DocumentChromeLayoutTests, CommandPaletteNavigation-
RegressionTests, SiblingSearchTests and DocumentQuickLookTests wait for the
app shell's controller (each group's test file lists them).
