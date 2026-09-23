import Darwin
import Foundation

/// Swift side of the `down-cli` suite: runs Downright's real `down`
/// (`target/downright-cli/release/down`, built by `just downright-cli`) on one
/// scenario in a fresh sandbox and dumps what it did. The Rust side
/// (`crates/conformance/src/dump/down_cli.rs`) runs `target/release/down` the
/// same way.
///
/// A scenario (`corpus/down-cli/*.json`):
///
///     {
///       "files": [{"path": "work/a.md", "text": "…"},       // UTF-8 text
///                 {"path": "work/b.md", "hex": "ff00"},     // raw bytes
///                 {"path": "work/c.md", "repeat": "a", "count": 10485761,
///                  "prefix": "…", "suffix": "…"},            // generated
///                 {"path": "work/d", "directory": true},
///                 {"path": "work/e.md", "symlink": "a.md"},
///                 {"path": "work/corpus", "copyTree": "corpus/generated/spec"},
///                 {"path": "work/f.md", "text": "", "mode": "000"}],
///       "cwd": "work",                  // relative to the sandbox (default)
///       "argv": ["check", "a.md"],
///       "argv0": "bin/down",            // default: the absolute bin/down path
///       "stdin": {"text": "…"} | {"hex": "…"} | {"repeat": "a", "count": N},
///                                       // absent: /dev/null
///       "env": {"NAME": "value"},
///       "stdoutFormat": "lines" | "json-lines",
///       "tempFiles": true,              // collect new stdin-*.md documents
///       "ignore": ["work/corpus"]       // left out of the snapshot
///     }
///
/// Every string in `files`, `argv`, `argv0`, `cwd` and `env` may name
/// `$SANDBOX`. The sandbox (under `target/down-cli-sandboxes`) holds `bin/down`
/// (a symbolic link to the binary, which is run through it), `home` (HOME and
/// CFFIXED_USER_HOME), `work` and `tmp` (TMPDIR). The environment is exactly
/// HOME, CFFIXED_USER_HOME, PATH, TMPDIR and the scenario's `env`. Files are
/// created with POSIX calls on the names' bytes as written (no Unicode
/// normalization); modes are applied after every file exists.
///
/// The dump holds the exit status (or signal), stdout and stderr, and a
/// snapshot of every item in the sandbox afterwards (relative path, type,
/// permission bits, contents; a file over 1 MiB as its size and FNV-1a
/// hash), skipping `bin`, `home/Library` and xcrun's
/// `tmp/xcrun_db*` caches. Text is split into lines on `\n`; bytes that are
/// not UTF-8 are dumped as hex. The sandbox path, also in its JSON-escaped
/// `\/` form, is replaced by `$SANDBOX` in all captured bytes. With
/// `"tempFiles"`, documents `down` wrote to `FileManager.temporaryDirectory`
/// `/Downright` (which ignores TMPDIR) are dumped with their UUID replaced by
/// `$UUID`, then deleted. A `copyTree` copies a directory of the repository
/// (regular files and directories, names byte for byte) and the dump records
/// a 64-bit FNV-1a digest of what it copied, so a Swift result cached before
/// the source changed shows up as a difference. No field is read from the
/// clock.
enum DownCLIDump {
    static func run(input: URL, flags: [String]) throws -> JSON {
        let data = try Data(contentsOf: input)
        guard let scenario = try JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            throw AppOracleError(description: "\(input.path): a scenario is a JSON object")
        }
        guard let root = repositoryRootDirectory() else {
            throw AppOracleError(description: "cannot find the repository root")
        }
        let binary = root + "/target/downright-cli/release/down"
        guard FileManager.default.isExecutableFile(atPath: binary) else {
            throw AppOracleError(description: "\(binary) is missing; run `just downright-cli`")
        }
        return try DownCLIScenario(scenario: scenario, root: root, binary: binary, side: "swift").run()
    }

    /// The directory holding `Cargo.toml` and `vendor/downright`, found upward
    /// from this binary.
    static func repositoryRootDirectory() -> String? {
        var directory = URL(fileURLWithPath: CommandLine.arguments[0]).resolvingSymlinksInPath().deletingLastPathComponent().path
        while directory != "/" && !directory.isEmpty {
            if FileManager.default.fileExists(atPath: directory + "/Cargo.toml"),
               FileManager.default.fileExists(atPath: directory + "/vendor/downright") {
                return directory
            }
            directory = (directory as NSString).deletingLastPathComponent
        }
        return nil
    }
}

private struct DownCLIScenario {
    let scenario: [String: Any]
    let root: String
    let binary: String
    let side: String

    func run() throws -> JSON {
        let parent = root + "/target/down-cli-sandboxes"
        mkdirs(parent)
        let sandbox = parent + "/\(side)-\(UUID().uuidString)"
        mkdirs(sandbox)
        defer { removeTree(sandbox) }
        for directory in ["bin", "home", "work", "tmp"] { mkdirs(sandbox + "/" + directory) }
        guard symlink(binary, sandbox + "/bin/down") == 0 else {
            throw AppOracleError(description: "cannot link \(binary)")
        }

        let substitute = { (text: String) -> String in replaceAll(text, "$SANDBOX", sandbox) }

        // Files.
        let files = scenario["files"] as? [[String: Any]] ?? []
        var copied: [(String, JSON)] = []
        for file in files {
            guard let relative = file["path"] as? String else { throw AppOracleError(description: "file without path") }
            let path = sandbox + "/" + substitute(relative)
            mkdirs((path as NSString).deletingLastPathComponent)
            if file["directory"] as? Bool == true {
                mkdirs(path)
            } else if let source = file["copyTree"] as? String {
                var hash: UInt64 = 0xcbf2_9ce4_8422_2325
                try copyTree(root + "/" + source, to: path, hash: &hash)
                copied.append((relative, .hex(hash)))
            } else if let target = file["symlink"] as? String {
                guard symlink(substitute(target), path) == 0 else { throw AppOracleError(description: "symlink \(path)") }
            } else {
                try writeBytes(bytes(file, substitute: substitute), to: path)
            }
        }
        for file in files {
            guard let mode = file["mode"] as? String, let relative = file["path"] as? String else { continue }
            _ = chmod(sandbox + "/" + substitute(relative), mode_t(strtol(mode, nil, 8)))
        }

        let cwd = sandbox + "/" + substitute(scenario["cwd"] as? String ?? "work")
        let argv0 = (scenario["argv0"] as? String).map(substitute) ?? sandbox + "/bin/down"
        let argv = [argv0] + (scenario["argv"] as? [String] ?? []).map(substitute)
        var environment = [
            "HOME": sandbox + "/home",
            "CFFIXED_USER_HOME": sandbox + "/home",
            "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
            "TMPDIR": sandbox + "/tmp/",
        ]
        for (key, value) in scenario["env"] as? [String: String] ?? [:] { environment[key] = substitute(value) }
        let stdin: [UInt8]? = try (scenario["stdin"] as? [String: Any]).map { try bytes($0, substitute: substitute) }

        let collectTemp = scenario["tempFiles"] as? Bool == true
        let tempDirectory = FileManager.default.temporaryDirectory.appendingPathComponent("Upleft").path
        let tempBefore = Set((try? FileManager.default.contentsOfDirectory(atPath: tempDirectory)) ?? [])

        let result = try spawnAndWait(executable: sandbox + "/bin/down", argv: argv, environment: environment, cwd: cwd, stdin: stdin)

        var members: [(String, JSON)] = []
        switch result.status {
        case .exit(let code): members.append(("status", .object([("exit", .int(Int(code)))])))
        case .signal(let signal): members.append(("status", .object([("signal", .int(Int(signal)))])))
        case .timeout: members.append(("status", .string("timeout")))
        }
        let masked = { (data: [UInt8]) -> [UInt8] in mask(data, sandbox: sandbox) }
        if scenario["stdoutFormat"] as? String == "json-lines" {
            members.append(("stdout", jsonLines(masked(result.stdout))))
        } else {
            members.append(("stdout", textOrHex(masked(result.stdout))))
        }
        members.append(("stderr", textOrHex(masked(result.stderr))))
        if !copied.isEmpty { members.append(("copied", .object(copied))) }
        let ignored = scenario["ignore"] as? [String] ?? []
        members.append(("files", .array(snapshot(sandbox, ignoring: ignored).map { item in
            var entry: [(String, JSON)] = [("path", .string(item.path)), ("type", .string(item.type)), ("mode", .string(item.mode))]
            if let target = item.target { entry.append(("target", textOrHex(masked(target)))) }
            if let contents = item.contents { entry.append(("contents", contents.count > 1_048_576 ? digest(contents) : textOrHex(masked(contents)))) }
            return .object(entry)
        })))
        if collectTemp {
            let after = (try? FileManager.default.contentsOfDirectory(atPath: tempDirectory)) ?? []
            var temp: [JSON] = []
            for name in after.sorted() where !tempBefore.contains(name) && name.hasPrefix("stdin-") {
                let path = tempDirectory + "/" + name
                let contents = (try? Data(contentsOf: URL(fileURLWithPath: path))).map { [UInt8]($0) } ?? []
                temp.append(.object([("name", .string(maskUUID(name))), ("contents", textOrHex(masked(contents)))]))
                try? FileManager.default.removeItem(atPath: path)
            }
            members.append(("tempFiles", .array(temp)))
        }
        return .object(members)
    }

    func bytes(_ spec: [String: Any], substitute: (String) -> String) throws -> [UInt8] {
        if let text = spec["text"] as? String { return Array(substitute(text).utf8) }
        if let hex = spec["hex"] as? String { return try hexBytes(hex) }
        if let unit = spec["repeat"] as? String, let count = spec["count"] as? Int {
            let prefix = Array(substitute(spec["prefix"] as? String ?? "").utf8)
            let suffix = Array(substitute(spec["suffix"] as? String ?? "").utf8)
            let piece = Array(unit.utf8)
            var out = prefix
            out.reserveCapacity(prefix.count + piece.count * count + suffix.count)
            for _ in 0..<count { out.append(contentsOf: piece) }
            return out + suffix
        }
        return []
    }
}

// MARK: - Sandbox

private func mkdirs(_ path: String) {
    var current = ""
    for component in path.split(separator: "/", omittingEmptySubsequences: true) {
        current += "/" + component
        _ = current.withCString { mkdir($0, 0o777) }
    }
}

private func writeBytes(_ bytes: [UInt8], to path: String) throws {
    let fd = path.withCString { open($0, O_WRONLY | O_CREAT | O_TRUNC, 0o666) }
    guard fd >= 0 else { throw AppOracleError(description: "cannot create \(path)") }
    defer { close(fd) }
    var offset = 0
    while offset < bytes.count {
        let written = bytes[offset...].withUnsafeBytes { write(fd, $0.baseAddress, $0.count) }
        guard written > 0 else { throw AppOracleError(description: "cannot write \(path)") }
        offset += written
    }
}

/// Copies the regular files and directories under `source` to
/// `destination`, in byte order of their names, folding each relative path
/// and its contents into a 64-bit FNV-1a hash.
private func copyTree(_ source: String, to destination: String, hash: inout UInt64, relative: String = "") throws {
    func fold(_ bytes: [UInt8]) {
        for byte in bytes {
            hash ^= UInt64(byte)
            hash = hash &* 0x0000_0100_0000_01b3
        }
    }
    mkdirs(destination)
    for name in (directoryEntries(source) ?? []).sorted(by: { Array($0.utf8).lexicographicallyPrecedes(Array($1.utf8)) }) {
        var info = stat()
        guard lstat(source + "/" + name, &info) == 0 else { continue }
        let path = relative.isEmpty ? name : relative + "/" + name
        if (info.st_mode & S_IFMT) == S_IFDIR {
            try copyTree(source + "/" + name, to: destination + "/" + name, hash: &hash, relative: path)
        } else if (info.st_mode & S_IFMT) == S_IFREG {
            let contents = [UInt8](try Data(contentsOf: URL(fileURLWithPath: source + "/" + name)))
            try writeBytes(contents, to: destination + "/" + name)
            fold(Array(path.utf8) + [0])
            fold(contents + [0])
        }
    }
}

/// Deletes a tree, first making every directory in it writable and searchable.
private func removeTree(_ path: String) {
    var info = stat()
    guard lstat(path, &info) == 0 else { return }
    if (info.st_mode & S_IFMT) == S_IFDIR {
        _ = chmod(path, 0o700)
        if let entries = directoryEntries(path) {
            for name in entries { removeTree(path + "/" + name) }
        }
        _ = rmdir(path)
    } else {
        _ = unlink(path)
    }
}

private func directoryEntries(_ path: String) -> [String]? {
    guard let directory = opendir(path) else { return nil }
    defer { closedir(directory) }
    var names: [String] = []
    while let entry = readdir(directory) {
        let name = withUnsafePointer(to: entry.pointee.d_name) {
            $0.withMemoryRebound(to: CChar.self, capacity: Int(MAXNAMLEN) + 1) { String(cString: $0) }
        }
        if name != "." && name != ".." { names.append(name) }
    }
    return names
}

private struct SnapshotItem {
    var path: String
    var type: String
    var mode: String
    var target: [UInt8]? = nil
    var contents: [UInt8]? = nil
}

/// Every item under the sandbox, by relative path in UTF-8 byte order.
private func snapshot(_ sandbox: String, ignoring ignored: [String]) -> [SnapshotItem] {
    var items: [SnapshotItem] = []
    func visit(_ relative: String) {
        let path = relative.isEmpty ? sandbox : sandbox + "/" + relative
        if relative == "bin" || relative == "home/Library" || ignored.contains(relative) { return }
        if Array(relative.utf8).starts(with: Array("tmp/xcrun_db".utf8)) { return }
        var info = stat()
        guard lstat(path, &info) == 0 else { return }
        let mode = String(Int(info.st_mode & 0o7777), radix: 8)
        switch info.st_mode & S_IFMT {
        case S_IFDIR:
            if !relative.isEmpty { items.append(SnapshotItem(path: relative, type: "directory", mode: mode)) }
            let permissions = info.st_mode & 0o7777
            let searchable = (permissions & 0o500) == 0o500
            if !searchable { _ = chmod(path, permissions | 0o500) }
            let names = directoryEntries(path) ?? []
            if !searchable { _ = chmod(path, permissions) }
            for name in names.sorted(by: { Array($0.utf8).lexicographicallyPrecedes(Array($1.utf8)) }) {
                visit(relative.isEmpty ? name : relative + "/" + name)
            }
        case S_IFLNK:
            var buffer = [CChar](repeating: 0, count: Int(PATH_MAX) + 1)
            let length = readlink(path, &buffer, buffer.count - 1)
            let target = length > 0 ? buffer[0..<length].map { UInt8(bitPattern: $0) } : []
            items.append(SnapshotItem(path: relative, type: "symlink", mode: mode, target: target))
        case S_IFREG:
            let permissions = info.st_mode & 0o7777
            let readable = (permissions & 0o400) != 0
            if !readable { _ = chmod(path, permissions | 0o400) }
            let contents = (try? Data(contentsOf: URL(fileURLWithPath: path))).map { [UInt8]($0) } ?? []
            if !readable { _ = chmod(path, permissions) }
            items.append(SnapshotItem(path: relative, type: "file", mode: mode, contents: contents))
        default:
            items.append(SnapshotItem(path: relative, type: "other", mode: mode))
        }
    }
    visit("")
    return items
}

// MARK: - Process

private enum SpawnStatus {
    case exit(Int32)
    case signal(Int32)
    case timeout
}

private struct SpawnResult {
    var status: SpawnStatus
    var stdout: [UInt8]
    var stderr: [UInt8]
}

private func readAll(_ fd: Int32) -> [UInt8] {
    var out: [UInt8] = []
    var buffer = [UInt8](repeating: 0, count: 65536)
    while true {
        let count = read(fd, &buffer, buffer.count)
        if count > 0 { out.append(contentsOf: buffer[0..<count]) }
        else if count < 0 && errno == EINTR { continue }
        else { break }
    }
    return out
}

private final class Box<T> {
    var value: T
    init(_ value: T) { self.value = value }
}

/// `posix_spawn` with an explicit argv (argv[0] included), environment,
/// working directory and pipes, SIGPIPE at its default action and an empty
/// signal mask; kills the child after 60 seconds.
private func spawnAndWait(executable: String, argv: [String], environment: [String: String], cwd: String, stdin: [UInt8]?) throws -> SpawnResult {
    var outPipe: [Int32] = [0, 0], errPipe: [Int32] = [0, 0], inPipe: [Int32] = [0, 0]
    guard pipe(&outPipe) == 0, pipe(&errPipe) == 0 else { throw AppOracleError(description: "pipe") }
    let stdinFD: Int32
    if stdin != nil {
        guard pipe(&inPipe) == 0 else { throw AppOracleError(description: "pipe") }
        stdinFD = inPipe[0]
    } else {
        stdinFD = open("/dev/null", O_RDONLY)
    }

    var actions: posix_spawn_file_actions_t? = nil
    posix_spawn_file_actions_init(&actions)
    defer { posix_spawn_file_actions_destroy(&actions) }
    posix_spawn_file_actions_adddup2(&actions, stdinFD, 0)
    posix_spawn_file_actions_adddup2(&actions, outPipe[1], 1)
    posix_spawn_file_actions_adddup2(&actions, errPipe[1], 2)
    _ = cwd.withCString { posix_spawn_file_actions_addchdir_np(&actions, $0) }

    var attributes: posix_spawnattr_t? = nil
    posix_spawnattr_init(&attributes)
    defer { posix_spawnattr_destroy(&attributes) }
    var defaults = sigset_t()
    sigemptyset(&defaults)
    sigaddset(&defaults, SIGPIPE)
    posix_spawnattr_setsigdefault(&attributes, &defaults)
    var mask = sigset_t()
    sigemptyset(&mask)
    posix_spawnattr_setsigmask(&attributes, &mask)
    posix_spawnattr_setflags(&attributes, Int16(POSIX_SPAWN_CLOEXEC_DEFAULT | POSIX_SPAWN_SETSIGDEF | POSIX_SPAWN_SETSIGMASK))

    let cArguments: [UnsafeMutablePointer<CChar>?] = argv.map { strdup($0) } + [nil]
    let cEnvironment: [UnsafeMutablePointer<CChar>?] = environment.sorted { $0.key < $1.key }.map { strdup("\($0.key)=\($0.value)") } + [nil]
    defer {
        cArguments.forEach { free($0) }
        cEnvironment.forEach { free($0) }
    }
    var pid: pid_t = 0
    let spawned = executable.withCString { posix_spawn(&pid, $0, &actions, &attributes, cArguments, cEnvironment) }
    close(outPipe[1]); close(errPipe[1]); close(stdinFD)
    guard spawned == 0 else { throw AppOracleError(description: "posix_spawn failed: \(spawned)") }

    let group = DispatchGroup()
    let stdoutBytes = Box<[UInt8]>([]), stderrBytes = Box<[UInt8]>([])
    let outReader = outPipe[0], errReader = errPipe[0]
    DispatchQueue.global().async(group: group) { stdoutBytes.value = readAll(outReader); close(outReader) }
    DispatchQueue.global().async(group: group) { stderrBytes.value = readAll(errReader); close(errReader) }
    if let stdin {
        let writer = inPipe[1]
        DispatchQueue.global().async(group: group) {
            var offset = 0
            while offset < stdin.count {
                let written = stdin[offset...].withUnsafeBytes { write(writer, $0.baseAddress, min($0.count, 65536)) }
                if written <= 0 { break }
                offset += written
            }
            close(writer)
        }
    }

    var status: Int32 = 0
    var timedOut = false
    let deadline = Date().addingTimeInterval(60)
    while true {
        let result = waitpid(pid, &status, WNOHANG)
        if result == pid { break }
        if result < 0 && errno != EINTR { break }
        if Date() > deadline {
            kill(pid, SIGKILL)
            timedOut = true
            _ = waitpid(pid, &status, 0)
            break
        }
        usleep(2_000)
    }
    group.wait()

    let spawnStatus: SpawnStatus
    if timedOut {
        spawnStatus = .timeout
    } else if (status & 0x7f) == 0 {
        spawnStatus = .exit((status >> 8) & 0xff)
    } else {
        spawnStatus = .signal(status & 0x7f)
    }
    return SpawnResult(status: spawnStatus, stdout: stdoutBytes.value, stderr: stderrBytes.value)
}

// MARK: - Text

private func hexBytes(_ hex: String) throws -> [UInt8] {
    let digits = Array(hex.utf8).filter { $0 != 0x20 }
    guard digits.count % 2 == 0 else { throw AppOracleError(description: "odd hex") }
    var out: [UInt8] = []
    var index = 0
    while index < digits.count {
        guard let byte = UInt8(String(decoding: digits[index..<index + 2], as: UTF8.self), radix: 16) else {
            throw AppOracleError(description: "bad hex")
        }
        out.append(byte)
        index += 2
    }
    return out
}

/// Byte-level replacement (no Unicode semantics).
private func replaceBytes(_ data: [UInt8], _ target: [UInt8], _ replacement: [UInt8]) -> [UInt8] {
    guard !target.isEmpty, data.count >= target.count else { return data }
    var out: [UInt8] = []
    out.reserveCapacity(data.count)
    var index = 0
    while index < data.count {
        if index + target.count <= data.count, data[index..<index + target.count].elementsEqual(target) {
            out.append(contentsOf: replacement)
            index += target.count
        } else {
            out.append(data[index])
            index += 1
        }
    }
    return out
}

private func replaceAll(_ text: String, _ target: String, _ replacement: String) -> String {
    String(decoding: replaceBytes(Array(text.utf8), Array(target.utf8), Array(replacement.utf8)), as: UTF8.self)
}

private func mask(_ data: [UInt8], sandbox: String) -> [UInt8] {
    let escaped = Array(sandbox.utf8).flatMap { $0 == UInt8(ascii: "/") ? [UInt8(ascii: "\\"), UInt8(ascii: "/")] : [$0] }
    return replaceBytes(replaceBytes(data, Array(sandbox.utf8), Array("$SANDBOX".utf8)), escaped, Array("$SANDBOX".utf8))
}

/// `stdin-<UUID>.md` → `stdin-$UUID.md`.
private func maskUUID(_ name: String) -> String {
    guard name.hasPrefix("stdin-"), name.hasSuffix(".md"), name.utf8.count == 6 + 36 + 3 else { return name }
    return "stdin-$UUID.md"
}

/// Lines split on `\n` when the bytes are UTF-8, else `{"hex": …}`.
private func textOrHex(_ data: [UInt8]) -> JSON {
    guard isValidUTF8(data) else {
        return .object([("hex", .string(data.map { String(format: "%02x", $0) }.joined()))])
    }
    return .array(data.split(separator: 0x0A, omittingEmptySubsequences: false).map { .string(String(decoding: $0, as: UTF8.self)) })
}

/// A file over 1 MiB is dumped as its size and 64-bit FNV-1a hash.
private func digest(_ data: [UInt8]) -> JSON {
    var hash: UInt64 = 0xcbf2_9ce4_8422_2325
    for byte in data {
        hash ^= UInt64(byte)
        hash = hash &* 0x0000_0100_0000_01b3
    }
    return .object([("size", .int(data.count)), ("fnv1a64", .hex(hash))])
}

/// Strict UTF-8 validation (the standard library's decoder).
private func isValidUTF8(_ bytes: [UInt8]) -> Bool {
    var decoder = UTF8()
    var iterator = bytes.makeIterator()
    while true {
        switch decoder.decode(&iterator) {
        case .scalarValue: continue
        case .emptyInput: return true
        case .error: return false
        }
    }
}

/// Each line parsed as JSON (compared structurally), else kept as text.
private func jsonLines(_ data: [UInt8]) -> JSON {
    .array(data.split(separator: 0x0A, omittingEmptySubsequences: false).map { line in
        let bytes = Array(line)
        if !bytes.isEmpty, let object = try? JSONSerialization.jsonObject(with: Data(bytes), options: [.fragmentsAllowed]) {
            return .object([("json", anyJSON(object))])
        }
        return .string(String(decoding: bytes, as: UTF8.self))
    })
}

/// A parsed JSON value with object members in UTF-8 byte order of their keys.
private func anyJSON(_ value: Any) -> JSON {
    switch value {
    case let dictionary as [String: Any]:
        return .object(dictionary.keys.sorted { Array($0.utf8).lexicographicallyPrecedes(Array($1.utf8)) }.map { ($0, anyJSON(dictionary[$0]!)) })
    case let array as [Any]:
        return .array(array.map(anyJSON))
    case let string as String:
        return .string(string)
    case let number as NSNumber:
        if CFGetTypeID(number) == CFBooleanGetTypeID() { return .bool(number.boolValue) }
        if CFNumberIsFloatType(number) { return .double(number.doubleValue) }
        return .int(number.intValue)
    default:
        return .null
    }
}
