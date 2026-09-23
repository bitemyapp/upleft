import Foundation
import CoreServices

// Loads an .mdimporter in this process through CFPlugIn, the way mdworker
// does, and runs it on one file. Nothing is registered with Spotlight.
let args = CommandLine.arguments
let bundleURL = URL(fileURLWithPath: args[1]) as CFURL
let file = args[2]
guard let plugIn = CFPlugInCreate(kCFAllocatorDefault, bundleURL) else { print("no plugin"); exit(1) }
let typeID = CFUUIDGetConstantUUIDWithBytes(kCFAllocatorDefault,0x8B,0x08,0xC4,0xBF,0x41,0x5B,0x11,0xD8,0xB3,0xF9,0x00,0x03,0x93,0x67,0x26,0xFC)!
let interfaceID = CFUUIDGetConstantUUIDWithBytes(kCFAllocatorDefault,0x6E,0xBC,0x27,0xC4,0x89,0x9C,0x11,0xD8,0x84,0xAE,0x00,0x03,0x93,0x67,0x26,0xFC)!
guard let factories = CFPlugInFindFactoriesForPlugInTypeInPlugIn(typeID, plugIn) as? [CFUUID], let factory = factories.first else { print("no factory"); exit(1) }
guard let raw = CFPlugInInstanceCreate(kCFAllocatorDefault, factory, typeID) else { print("no instance"); exit(1) }
let iunknown = raw.assumingMemoryBound(to: UnsafeMutablePointer<IUnknownVTbl>.self)
var interface: LPVOID? = nil
let hr = iunknown.pointee.pointee.QueryInterface(raw, CFUUIDGetUUIDBytes(interfaceID), &interface)
print("QueryInterface", hr)
_ = iunknown.pointee.pointee.Release(raw)
guard hr == 0, let importerRaw = interface else { exit(1) }
let importer = importerRaw.assumingMemoryBound(to: UnsafeMutablePointer<MDImporterInterfaceStruct>.self)
let attributes = NSMutableDictionary()
let ok = importer.pointee.pointee.ImporterImportData(importerRaw, attributes as CFMutableDictionary, "net.daringfireball.markdown" as CFString, file as CFString)
print("import", ok)
for key in (attributes.allKeys as! [String]).sorted() { print(key, "=", String(describing: attributes[key]!).replacingOccurrences(of: "\n", with: " ")) }
let released = importer.pointee.pointee.Release(importerRaw)
print("released", released)
