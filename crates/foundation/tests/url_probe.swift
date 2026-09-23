// Records Swift `URL` behaviour for tests/url.rs. Run from this directory:
//   swiftc -O url_probe.swift -o /tmp/url_probe && /tmp/url_probe > data/url.json
// It builds the fixture tree under /tmp/upleft-url-fixture (the Rust test
// builds the same tree) and prints one JSON object per case.
import Foundation

let root = "/tmp/upleft-url-fixture"
let fm = FileManager.default
try? fm.removeItem(atPath: root)
for dir in ["dir", "dir/sub", "caf\u{e9}", "sp ace", "Docs.md"] {
    try! fm.createDirectory(atPath: root + "/" + dir, withIntermediateDirectories: true)
}
for file in ["file.md", "dir/a.md", "caf\u{e9}/n\u{f6}te.md", "sp ace/x y.markdown", "archive.tar.gz", ".hidden", "noext"] {
    fm.createFile(atPath: root + "/" + file, contents: Data("x".utf8))
}
try! fm.createSymbolicLink(atPath: root + "/link", withDestinationPath: root + "/dir")
fm.changeCurrentDirectoryPath(root)

let paths = [
    "/tmp/upleft-url-fixture", "/tmp/upleft-url-fixture/", "/tmp/upleft-url-fixture/dir", "/tmp/upleft-url-fixture/dir/",
    "/tmp/upleft-url-fixture/dir//", "/tmp/upleft-url-fixture/file.md", "/tmp/upleft-url-fixture/./dir/../file.md",
    "/tmp/upleft-url-fixture/link/a.md", "/tmp/upleft-url-fixture/link", "/private/tmp/upleft-url-fixture/file.md",
    "/tmp/upleft-url-fixture/caf\u{e9}/n\u{f6}te.md", "/tmp/upleft-url-fixture/cafe\u{301}", "/tmp/upleft-url-fixture/sp ace/x y.markdown",
    "/tmp/upleft-url-fixture/archive.tar.gz", "/tmp/upleft-url-fixture/.hidden", "/tmp/upleft-url-fixture/noext",
    "/tmp/upleft-url-fixture/Docs.md", "/tmp/upleft-url-fixture/missing.md", "/tmp/upleft-url-fixture/missing/",
    "/tmp/upleft-url-fixture//dir//a.md", "/tmp/upleft-url-fixture/..", "/tmp/upleft-url-fixture/dir/..",
    "/", "//", "/tmp", "file.md", "dir", "./dir/a.md", "dir/../file.md", "../upleft-url-fixture/file.md", "", ".", "..",
    "~", "~/Library", "~root/x", "a%20b.md", "q?x#y.md", "/tmp/upleft-url-fixture/a:b;c=d&e+f$g,h@i!j'k(l)m*n~o.md",
    "/tmp/upleft-url-fixture/\u{1F600}.md", "/tmp/upleft-url-fixture/\u{212B}.md", "/tmp/upleft-url-fixture/\u{FB01}.md",
    "/tmp/upleft-url-fixture/x.", "/tmp/upleft-url-fixture/x..md", "/tmp/upleft-url-fixture/.md", "/tmp/upleft-url-fixture/a.b/c",
]
let components = ["c.md", "dir", "dir/", "sub/x.md", "/lead", "", "..", ".", "caf\u{e9}", "n\u{f6}te.md", "a b", "x%2Fy", "trail/"]

func esc(_ s: String) -> String {
    var out = "\""
    for u in s.unicodeScalars {
        switch u {
        case "\"": out += "\\\""
        case "\\": out += "\\\\"
        default:
            if u.value < 0x20 { out += String(format: "\\u%04x", u.value) } else { out.unicodeScalars.append(u) }
        }
    }
    return out + "\""
}
func describe(_ u: URL) -> String {
    "{\"abs\":\(esc(u.absoluteString)),\"path\":\(esc(u.path)),\"dir\":\(u.hasDirectoryPath)}"
}

var lines: [String] = []
for p in paths {
    let u = URL(fileURLWithPath: p)
    var fields = [
        "\"init\":\(describe(u))",
        "\"initDir\":\(describe(URL(fileURLWithPath: p, isDirectory: true)))",
        "\"initFile\":\(describe(URL(fileURLWithPath: p, isDirectory: false)))",
        "\"last\":\(esc(u.lastPathComponent))",
        "\"ext\":\(esc(u.pathExtension))",
        "\"components\":[\(u.pathComponents.map(esc).joined(separator: ","))]",
        "\"deleteLast\":\(describe(u.deletingLastPathComponent()))",
        "\"deleteExt\":\(describe(u.deletingPathExtension()))",
        "\"appendExt\":\(describe(u.appendingPathExtension("md")))",
        "\"standardized\":\(describe(u.standardizedFileURL))",
        "\"resolved\":\(describe(u.resolvingSymlinksInPath()))",
    ]
    var appended: [String] = []
    for c in components {
        appended.append("{\"component\":\(esc(c)),\"plain\":\(describe(u.appendingPathComponent(c))),\"dir\":\(describe(u.appendingPathComponent(c, isDirectory: true))),\"file\":\(describe(u.appendingPathComponent(c, isDirectory: false)))}")
    }
    fields.append("\"append\":[\(appended.joined(separator: ","))]")
    lines.append("{\"input\":\(esc(p)),\(fields.joined(separator: ","))}")
}
print("[\n" + lines.joined(separator: ",\n") + "\n]")
