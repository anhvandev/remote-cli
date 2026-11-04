// PROTOTYPE, throwaway. Question: can a process capture the screen and inject a key
// (a) from a root LoginWindow LaunchAgent at the login window, (b) from a user agent while the screen is locked?
// Build: swiftc -O -target $(uname -m)-apple-macos14.0 probe.swift -o lockscreen-probe
// Run:   ./lockscreen-probe [outDir=/tmp/lockscreen-probe] [rounds=30]
import Foundation
import CoreGraphics
import CoreImage
import CoreVideo
import ImageIO
import UniformTypeIdentifiers
import ScreenCaptureKit

let args = CommandLine.arguments
let outDir = args.count > 1 ? args[1] : "/tmp/lockscreen-probe"
let rounds = args.count > 2 ? Int(args[2]) ?? 30 : 30
let tag = "pid\(getpid())-uid\(getuid())"
try? FileManager.default.createDirectory(atPath: outDir, withIntermediateDirectories: true,
                                         attributes: [.posixPermissions: 0o777])

func log(_ s: String) {
    let line = "\(ISO8601DateFormatter().string(from: Date())) [\(tag)] \(s)\n"
    FileHandle.standardError.write(line.data(using: .utf8)!)
    let url = URL(fileURLWithPath: "\(outDir)/probe.log")
    if let h = try? FileHandle(forWritingTo: url) { h.seekToEndOfFile(); h.write(line.data(using: .utf8)!); h.closeFile() }
    else { FileManager.default.createFile(atPath: url.path, contents: line.data(using: .utf8), attributes: [.posixPermissions: 0o666]) }
}

func save(_ img: CGImage?, _ name: String) -> String {
    guard let img else { return "nil-image" }
    let url = URL(fileURLWithPath: "\(outDir)/\(name).jpg")
    guard let dst = CGImageDestinationCreateWithURL(url as CFURL, UTType.jpeg.identifier as CFString, 1, nil)
    else { return "dest-fail" }
    CGImageDestinationAddImage(dst, img, [kCGImageDestinationLossyCompressionQuality: 0.5] as CFDictionary)
    return CGImageDestinationFinalize(dst) ? "\(url.lastPathComponent) \(img.width)x\(img.height)" : "write-fail"
}

// A: CGDisplayStream, the API RustDesk's scrap uses (obsoleted in the 15 SDK, hence the 14.0 target).
func grabDisplayStream() -> CGImage? {
    let d = CGMainDisplayID()
    var result: CGImage?
    let sem = DispatchSemaphore(value: 0)
    guard let stream = CGDisplayStream(
        dispatchQueueDisplay: d, outputWidth: CGDisplayPixelsWide(d), outputHeight: CGDisplayPixelsHigh(d),
        pixelFormat: Int32(kCVPixelFormatType_32BGRA), properties: nil, queue: DispatchQueue(label: "ds"),
        handler: { status, _, surface, _ in
            guard status == .frameComplete, let surface, result == nil else { return }
            let ci = CIImage(ioSurface: surface)
            result = CIContext().createCGImage(ci, from: ci.extent)
            sem.signal()
        }) else { return nil }
    stream.start()
    _ = sem.wait(timeout: .now() + 3)
    stream.stop()
    return result
}

// B: ScreenCaptureKit, the supported modern API.
func grabSCK() async -> (CGImage?, String) {
    do {
        let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: true)
        guard let display = content.displays.first else { return (nil, "no display") }
        let cfg = SCStreamConfiguration()
        cfg.width = display.width; cfg.height = display.height
        let img = try await SCScreenshotManager.captureImage(
            contentFilter: SCContentFilter(display: display, excludingWindows: []), configuration: cfg)
        return (img, "ok")
    } catch { return (nil, "\(error)") }
}

// Types "a" (virtual keycode 0); on a login/lock screen it should show up as a password dot.
func pressKey(_ state: CGEventSourceStateID, _ tap: CGEventTapLocation) -> String {
    let src = CGEventSource(stateID: state)
    for down in [true, false] {
        let ev = CGEvent(keyboardEventSource: src, virtualKey: 0, keyDown: down)
        ev?.flags = [] // enigo clears flags too: macOS can carry stale modifier flags over at the lock screen
        ev?.post(tap: tap)
    }
    return "posted"
}

final class Box: @unchecked Sendable { var v = "" }

// Runs a step on its own thread, so a hung step is logged as TIMEOUT and skipped instead of stalling the probe.
func step(_ name: String, timeout: Double = 5, _ f: @escaping @Sendable () -> String) {
    let sem = DispatchSemaphore(value: 0), box = Box()
    Thread { box.v = f(); sem.signal() }.start()
    log("  \(name): " + (sem.wait(timeout: .now() + timeout) == .success ? box.v : "TIMEOUT (hung)"))
}

func blocking(_ f: @escaping @Sendable () async -> String) -> String {
    let sem = DispatchSemaphore(value: 0), box = Box()
    Task.detached { box.v = await f(); sem.signal() }
    sem.wait()
    return box.v
}

log("start rounds=\(rounds) screenCaptureAccess=\(CGPreflightScreenCaptureAccess()) postEventAccess=\(CGPreflightPostEventAccess())")
for i in 1...rounds {
    let s = CGSessionCopyCurrentDictionary() as? [String: Any]
    log("round \(i) session=\(s?["kCGSSessionUserNameKey"] ?? "none") locked=\(s?["CGSSessionScreenIsLocked"] ?? "no") onConsole=\(s?["kCGSSessionOnConsoleKey"] ?? "?")")
    if ProcessInfo.processInfo.environment["NOKEY"] == nil { // NOKEY=1 for a baseline run in a normal session
        // One key variant per round, so a new password dot can be traced back to the variant that produced it.
        switch (i - 1) % 3 {
        case 0: step("key(combined->hid, enigo)") { pressKey(.combinedSessionState, .cghidEventTap) }
        case 1: step("key(combined->session)") { pressKey(.combinedSessionState, .cgSessionEventTap) }
        default: step("key(hidSystem->hid)") { pressKey(.hidSystemState, .cghidEventTap) }
        }
    }
    sleep(1)
    step("displayStream") { save(grabDisplayStream(), "\(tag)-\(i)-ds") }
    step("sck", timeout: 10) { blocking { let (img, err) = await grabSCK(); return err == "ok" ? save(img, "\(tag)-\(i)-sck") : err } }
    sleep(9)
}
log("done")
