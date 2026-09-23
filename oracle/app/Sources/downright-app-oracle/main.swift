import AppKit
@testable import DownrightApp

// downright-app-oracle — the Swift reference for Upleft's app-layer suites.
//
//   downright-app-oracle <command> <input> <out.json> [flags…]
//
//   html-export  <file.md>        HTMLExporter output (HTMLExportDump.swift)
//   spotlight    <file.md>        Spotlight metadata (SpotlightDump.swift)
//   down-cli     <scenario.json>  runs the real `down` in a sandbox (DownCLIDump.swift)
//   workspace    <queries.json>   WorkspaceIndex / LinkGraph / Search (WorkspaceDump.swift)
//   find         <queries.json>   FindEngine (FindDump.swift)
//   palette      <queries.json>   CommandPaletteModel / QuickOpenProviders (PaletteDump.swift)
//   formats      <script.json>    persisted formats (FormatsDump.swift)
//   updater      <case.json>      UpdateStateMachine and appcast parsing (UpdaterDump.swift)
//   local-ai     <case.json>      LocalAI prompt and result shaping (LocalAIDump.swift)
//   bench-export    <file.md>     HTMLExporter timings (AppBench.swift)
//   bench-workspace <folder>      WorkspaceIndex, graph and search timings (AppBench.swift)
//   bench-find      <file.md>     FindEngine and FindSession timings (AppBench.swift)
//   app-window       <scenario.json> <out.png> [--layout out.json]
//                                 a real app window, captured off-screen (AppWindowCapture.swift)
//   bench-app-window <scenario.json> document open and mode-switch timings (AppWindowCapture.swift)
//   panel        <scenario.json>  a panel built off-screen and captured (Panels/PanelHarness.swift)
//   panel-model  <scenario.json>  panels built windowless, laid out and dumped (Panels/PanelHarness.swift)
//   bench-panel  <scenario.json>  panel build and layout timings (Panels/PanelBench.swift)
//
// Each command parses its own flags. `upleft-oracle` (crates/conformance)
// takes identical arguments and writes identical formats. The runner selects
// this binary for suites marked `"oracle": "app"` in conformance/suites.json.

func usage() -> Never {
    FileHandle.standardError.write("usage: downright-app-oracle <command> <input> <out.json> [flags…]\n".data(using: .utf8)!)
    exit(64)
}

func write(_ json: JSON, to path: String) throws {
    try json.text.write(toFile: path, atomically: true, encoding: .utf8)
}

/// The repository root: the nearest directory above this binary that holds
/// `Cargo.toml` and `vendor/downright`. (`target/app-oracle/release` is a
/// symbolic link into SwiftPM's `out/Products/Release`, so a fixed number of
/// components is wrong.)
let repositoryRoot: URL = {
    var directory = URL(fileURLWithPath: CommandLine.arguments[0]).resolvingSymlinksInPath().deletingLastPathComponent().path
    while directory != "/" && !directory.isEmpty {
        if FileManager.default.fileExists(atPath: directory + "/Cargo.toml"),
           FileManager.default.fileExists(atPath: directory + "/vendor/downright") {
            return URL(fileURLWithPath: directory, isDirectory: true)
        }
        directory = (directory as NSString).deletingLastPathComponent
    }
    return URL(fileURLWithPath: FileManager.default.currentDirectoryPath, isDirectory: true)
}()

struct AppOracleError: Error, CustomStringConvertible {
    var description: String
}

let arguments = CommandLine.arguments
guard arguments.count >= 4 else { usage() }
let command = arguments[1]
let input = URL(fileURLWithPath: arguments[2])
let output = arguments[3]
let flags = Array(arguments.dropFirst(4))

do {
    switch command {
    case "html-export": try write(HTMLExportDump.run(input: input, flags: flags), to: output)
    case "spotlight": try write(SpotlightDump.run(input: input, flags: flags), to: output)
    case "down-cli": try write(DownCLIDump.run(input: input, flags: flags), to: output)
    case "workspace": try write(WorkspaceDump.run(input: input, flags: flags), to: output)
    case "find": try write(FindDump.run(input: input, flags: flags), to: output)
    case "palette": try write(PaletteDump.run(input: input, flags: flags), to: output)
    case "formats": try write(FormatsDump.run(input: input, flags: flags), to: output)
    case "updater": try write(UpdaterDump.run(input: input, flags: flags), to: output)
    case "local-ai": try write(LocalAIDump.run(input: input, flags: flags), to: output)
    case "bench-export": try write(AppBench.export(input: input), to: output)
    case "bench-workspace": try write(MainActor.assumeIsolated { try AppBench.workspace(folder: input) }, to: output)
    case "bench-find": try write(AppBench.find(input: input), to: output)
    case "app-window":
        try MainActor.assumeIsolated {
            try AppWindowSession.run(input: input, output: output, flags: flags, repositoryRoot: repositoryRoot)
        }
    case "bench-app-window":
        try MainActor.assumeIsolated {
            try AppWindowBench.run(input: input, output: output, repositoryRoot: repositoryRoot)
        }
    case "panel": try MainActor.assumeIsolated { try PanelCaptureSession.run(input: input, output: output, flags: flags) }
    case "panel-model": try write(MainActor.assumeIsolated { try PanelModelDump.run(input: input, flags: flags) }, to: output)
    default: usage()
    }
} catch {
    FileHandle.standardError.write("\(error)\n".data(using: .utf8)!)
    exit(1)
}
