import AppKit
import CryptoKit

// A bounded, standalone custom-control laboratory. Guest rows remain the oracle;
// AppKit rasterization is a preview, never a numerical conformance result.
typealias JSON = [String: Any]
struct CanvasError: Error, CustomStringConvertible { let description: String; init(_ s: String) { description = s } }
func jsonData(_ value: Any) throws -> Data { try JSONSerialization.data(withJSONObject: value, options: [.sortedKeys]) }
func dictionary(_ data: Data) throws -> JSON {
    guard let value = try JSONSerialization.jsonObject(with: data) as? JSON else { throw CanvasError("Expected JSON object") }
    return value
}
func writeJSON(_ value: Any, _ url: URL) throws { try jsonData(value).write(to: url, options: .atomic) }
func number(_ value: Any?) -> Int { (value as? NSNumber)?.intValue ?? 0 }
func digest(_ url: URL) throws -> String {
    let file = try FileHandle(forReadingFrom: url); defer { try? file.close() }
    var hash = SHA256()
    while let data = try file.read(upToCount: 1 << 20), !data.isEmpty { hash.update(data: data) }
    return hash.finalize().map { String(format: "%02x", $0) }.joined()
}
func floats(_ value: Any?, count: Int) throws -> [CGFloat] {
    guard let words = value as? [NSNumber], words.count == count else { throw CanvasError("Bad float vector") }
    let result = words.map { CGFloat(Float(bitPattern: $0.uint32Value)) }
    guard result.allSatisfy({ $0.isFinite && abs($0) <= 1_000_000 }) else { throw CanvasError("Nonfinite or excessive drawing coordinate") }
    return result
}
func color(_ value: Any?) throws -> NSColor {
    let c = try floats(value, count: 4) // Drawbot colors are RGBA, unlike PF_PixelFloat.
    return NSColor(srgbRed: c[0], green: c[1], blue: c[2], alpha: c[3])
}
func rectangle(_ value: Any?) throws -> NSRect {
    let v = try floats(value, count: 4)
    guard v[2] >= 0 && v[3] >= 0 else { throw CanvasError("Negative rectangle size") }
    return NSRect(x: v[0], y: v[1], width: v[2], height: v[3])
}

final class DrawObject {
    var color = NSColor.black
    var size: CGFloat = 1
    var path = NSBezierPath()
    var references = 1
}
final class Rasterizer {
    var objects: [Int: DrawObject] = [:]
    // Keep objects across callbacks; retained objects may outlive one draw.
    func render(_ calls: [JSON], into image: NSImage, origin: NSPoint) throws {
        image.lockFocusFlipped(true)
        defer { image.unlockFocus() }
        NSGraphicsContext.saveGraphicsState()
        defer { NSGraphicsContext.restoreGraphicsState() }
        let transform = NSAffineTransform()
        transform.translateX(by: -origin.x, yBy: -origin.y); transform.concat()
        for c in calls {
            let name = c["call"] as? String ?? ""
            let id = number(c["object"]), pathID = number(c["path"])
            func object(_ id: Int) throws -> DrawObject {
                guard let o = objects[id] else { throw CanvasError("Missing drawing object \(id)") }; return o
            }
            switch name {
            case "NewPen", "NewBrush", "NewDefaultFont", "NewPath":
                let o = DrawObject()
                if name == "NewPen" || name == "NewBrush" { o.color = try color(c["color_bits"]) }
                if name == "NewPen" || name == "NewDefaultFont" {
                    o.size = try floats([c["size_bits"]!], count: 1)[0]
                    guard o.size > 0 && o.size <= 4096 else { throw CanvasError("Invalid pen/font size") }
                }
                objects[id] = o
            case "RetainObject": try object(id).references += 1
            case "ReleaseObject":
                let o = try object(id); o.references -= 1
                if o.references == 0 { objects.removeValue(forKey: id) }
            case "MoveTo", "LineTo":
                let p = try floats(c["point_bits"], count: 2), o = try object(pathID)
                let point = NSPoint(x: p[0], y: p[1])
                if name == "MoveTo" { o.path.move(to: point) } else { o.path.line(to: point) }
            case "AddRect": try object(pathID).path.appendRect(rectangle(c["rect_bits"]))
            case "Close": try object(pathID).path.close()
            case "PaintRect": try color(c["color_bits"]).setFill(); try rectangle(c["rect_bits"]).fill()
            case "FillPath", "StrokePath":
                let o = try object(id), p = try object(pathID).path
                o.color.set()
                if name == "FillPath" {
                    let rule = number(c["fill_type"])
                    guard rule == 0 || rule == 1 else { throw CanvasError("Unsupported fill rule \(rule)") }
                    p.windingRule = rule == 0 ? .evenOdd : .nonZero
                    p.fill()
                } else { p.lineWidth = o.size; p.stroke() }
            case "DrawString":
                let b = try object(number(c["brush"])), f = try object(number(c["font"]))
                let p = try floats(c["origin_bits"], count: 2)
                guard let units = c["utf16"] as? [UInt16] else { throw CanvasError("Invalid UTF16") }
                // AppKit measures and lays out the preview only; guest font metrics
                // are not fabricated. Drawbot defines origin on the text baseline.
                let alignment = number(c["alignment"]), truncation = number(c["truncation"])
                guard (0...2).contains(alignment) && (0...2).contains(truncation) else {
                    throw CanvasError("Unsupported text alignment/path ellipsis")
                }
                let font = NSFont.systemFont(ofSize: f.size)
                let attributes: [NSAttributedString.Key: Any] = [.font: font, .foregroundColor: b.color]
                var text = String(decoding: units, as: UTF16.self)
                let width = try floats([c["truncation_width_bits"]!], count: 1)[0]
                if truncation != 0 {
                    guard width >= 0 else { throw CanvasError("Negative text width") }
                    let suffix = truncation == 2 ? "…" : ""
                    if (text as NSString).size(withAttributes: attributes).width > width {
                        while !text.isEmpty && ((text + suffix) as NSString).size(withAttributes: attributes).width > width { text.removeLast() }
                        text += suffix
                    }
                }
                let size = (text as NSString).size(withAttributes: attributes)
                let offset = alignment == 1 ? size.width / 2 : (alignment == 2 ? size.width : 0)
                (text as NSString).draw(at: NSPoint(x: p[0] - offset, y: p[1] - font.ascender), withAttributes: attributes)
            case "SetAntiAliasPolicy":
                let policy = number(c["policy"])
                guard (0...2).contains(policy) else { throw CanvasError("Unsupported antialias policy") }
                NSGraphicsContext.current?.shouldAntialias = policy != 0
            case "PF_GetDrawingReference", "GetSupplier", "GetSurface", "PF_AppGetColor", "PF_AppColorPickerDialog", "PF_InvalidateRect": break
            default:
                // Non-drawing UPDATE/stream calls need no rasterization.
                if name.hasPrefix("PF_") || name.hasPrefix("AEGP_") { break }
                throw CanvasError("Unsupported recorded call: \(name)")
            }
        }
    }
}

final class Session {
    let process = Process(), input = Pipe(), output = Pipe()
    let directory: URL, worker: URL, plugin: URL
    var request: JSON, operations: [JSON] = [], buffer = Data()
    let trace: FileHandle, errors: FileHandle
    var count = 0, traceBytes = 0
    var monitor: DispatchSourceTimer?
    var capture: JSON = [:]
    var loadedPluginHashes = Set<String>()
    init(worker: URL, plugin: URL, directory: URL, request: JSON) throws {
        self.worker = worker; self.plugin = plugin; self.directory = directory; self.request = request
        guard !FileManager.default.fileExists(atPath: directory.path) else { throw CanvasError("Output directory already exists") }
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        // Initial operations are sent through the same stream, not hidden from recording.
        for key in ["operations", "updates", "events"] {
            guard (request[key] as? [Any] ?? []).isEmpty else { throw CanvasError("Canvas header must have no operations, updates or events") }
        }
        try writeJSON(request, directory.appendingPathComponent("header.json"))
        let traceURL = directory.appendingPathComponent("trace.jsonl"), logURL = directory.appendingPathComponent("worker.log")
        FileManager.default.createFile(atPath: traceURL.path, contents: nil)
        FileManager.default.createFile(atPath: logURL.path, contents: nil)
        trace = try FileHandle(forWritingTo: traceURL); errors = try FileHandle(forWritingTo: logURL)
        capture = ["worker_sha256": try digest(worker), "plugin_sha256_at_launch": try digest(plugin),
                   "canvas_sha256": try digest(URL(fileURLWithPath: CommandLine.arguments[0])),
                   "input_source": CommandLine.arguments.count == 6 ? "AppKit NSEvent injection" : "desktop AppKit input",
                   "exercise": CommandLine.arguments.count == 6 ? CommandLine.arguments[5] : "none",
                   "identity_note": "worker hash observed before launch; plugin loaded-byte identity is in trace rows",
                   "clean_close": false]
        try writeJSON(capture, directory.appendingPathComponent("capture.json"))
        process.executableURL = URL(fileURLWithPath: CommandLine.arguments[0]).standardizedFileURL
        process.arguments = ["--worker-child", worker.path, plugin.path, directory.appendingPathComponent("header.json").path]
        process.standardInput = input; process.standardOutput = output; process.standardError = errors
        var env = ProcessInfo.processInfo.environment; env["TMPDIR"] = directory.path; process.environment = env
        try process.run()
        // Parent must not retain the child end: EOF must be observable on crashes.
        try output.fileHandleForWriting.close(); try input.fileHandleForReading.close()
        let monitor = DispatchSource.makeTimerSource(queue: .global())
        monitor.schedule(deadline: .now(), repeating: .milliseconds(100))
        monitor.setEventHandler { [process] in
            guard process.isRunning else { return }
            var info = proc_taskinfo()
            let size = proc_pidinfo(process.processIdentifier, PROC_PIDTASKINFO, 0, &info, Int32(MemoryLayout<proc_taskinfo>.size))
            if size == MemoryLayout<proc_taskinfo>.size && info.pti_resident_size > 1 << 30 {
                kill(-process.processIdentifier, SIGKILL); kill(process.processIdentifier, SIGKILL)
            }
        }
        monitor.resume(); self.monitor = monitor
    }
    func exchange(_ operation: JSON) throws -> JSON {
        guard count < 256 else { throw CanvasError("256-operation limit reached; close and start a new session") }
        operations.append(operation)
        var saved = request; saved["operations"] = operations
        try writeJSON(saved, directory.appendingPathComponent("session.json"))
        var data = try jsonData(operation); data.append(10)
        try input.fileHandleForWriting.write(contentsOf: data)
        // Interactive-only deadline, not a discovery gate. Timer never waits on the UI queue.
        let timer = DispatchSource.makeTimerSource(queue: .global())
        timer.schedule(deadline: .now() + 30)
        timer.setEventHandler { [process] in if process.isRunning { kill(-process.processIdentifier, SIGKILL); kill(process.processIdentifier, SIGKILL) } }
        timer.resume(); defer { timer.cancel() }
        while !buffer.contains(10) {
            // Foundation read(upToCount:) can wait to fill the requested size
            // on a pipe. POSIX read returns the currently available response chunk.
            var chunk = [UInt8](repeating: 0, count: 65536)
            let size = Darwin.read(output.fileHandleForReading.fileDescriptor, &chunk, chunk.count)
            if size < 0 && errno == EINTR { continue }
            guard size > 0 else {
                throw CanvasError("Worker closed stdout; see worker.log")
            }
            buffer.append(contentsOf: chunk.prefix(size))
            guard buffer.count <= 32 << 20 else { throw CanvasError("Worker response exceeds 32 MiB") }
        }
        let end = buffer.firstIndex(of: 10)!, line = buffer.prefix(through: end)
        traceBytes += line.count
        guard traceBytes <= 128 << 20 else { throw CanvasError("Trace exceeds 128 MiB") }
        try trace.write(contentsOf: line)
        let row = try dictionary(Data(line)); buffer.removeSubrange(...end); count += 1
        if let hash = row["plugin_sha256"] as? String { loadedPluginHashes.insert(hash) }
        guard let result = row["event"] as? JSON, number(result["selector_error"]) == 0 else { throw CanvasError("Invalid worker event response") }
        return row
    }
    func close() throws {
        try? input.fileHandleForWriting.close()
        let timer = DispatchSource.makeTimerSource(queue: .global())
        timer.schedule(deadline: .now() + 5)
        timer.setEventHandler { [process] in if process.isRunning { kill(-process.processIdentifier, SIGKILL); kill(process.processIdentifier, SIGKILL) } }
        timer.resume(); process.waitUntilExit(); timer.cancel(); monitor?.cancel()
        kill(-process.processIdentifier, SIGKILL)
        try trace.close(); try errors.close()
        capture["worker_sha256_at_close"] = (try? digest(worker)) ?? "unavailable"
        capture["plugin_sha256_at_close"] = (try? digest(plugin)) ?? "unavailable"
        capture["loaded_plugin_sha256"] = loadedPluginHashes.sorted()
        capture["clean_close"] = process.terminationStatus == 0
        capture["returncode"] = process.terminationStatus
        capture["operations"] = count
        try writeJSON(capture, directory.appendingPathComponent("capture.json"))
        guard process.terminationStatus == 0 else { throw CanvasError("Worker exit \(process.terminationStatus); see worker.log") }
    }
}

final class Canvas: NSView {
    var eventTemplate: JSON = [:]
    var image: NSImage!
    var send: ((JSON) -> Void)?
    var activeDrag = false, ready = false, needsGuestDraw = true
    var lastPoint = NSPoint.zero
    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
    override func draw(_ dirtyRect: NSRect) {
        if needsGuestDraw {
            needsGuestDraw = false
            DispatchQueue.main.async { [weak self] in
                guard let self = self else { return }; self.send?(self.event(4, nil))
            }
        }
        NSColor.darkGray.setFill(); bounds.fill()
        image?.draw(in: bounds, from: .zero, operation: .sourceOver, fraction: 1, respectFlipped: true, hints: nil)
    }
    func event(_ kind: Int, _ input: NSEvent?, last: Bool = false) -> JSON {
        var e = eventTemplate
        if let input = input, input.type != .keyDown { lastPoint = convert(input.locationInWindow, from: nil) }
        let frame = e["frame"] as! [Int]
        e["event"] = kind; e["x"] = frame[0] + Int(lastPoint.x.rounded(.towardZero)); e["y"] = frame[1] + Int(lastPoint.y.rounded(.towardZero))
        e["last"] = last; e["clicks"] = max(1, input?.clickCount ?? 1)
        return ["type": "event", "event": e]
    }
    override func mouseDown(with e: NSEvent) {
        guard ready else { return }; window?.makeFirstResponder(self); send?(event(2, e))
    }
    override func mouseDragged(with e: NSEvent) { guard ready else { return }; send?(event(3, e)) }
    override func mouseUp(with e: NSEvent) { guard ready else { return }; send?(event(3, e, last: true)) }
    override func keyDown(with e: NSEvent) {
        guard ready else { return }
        // PF printable keycode = unshifted uppercase UTF16 | 0x80000000.
        // Restrict to ASCII until non-US/IME handling has an ABI witness.
        guard let text = e.characters(byApplyingModifiers: [])?.uppercased(), text.utf16.count == 1,
              let unit = text.utf16.first, unit < 128 else { NSSound.beep(); return }
        let controls: [UInt16: UInt32] = [32: 1, 127: 2, 9: 3, 13: 4, 3: 5, 27: 6]
        let code = controls[unit] ?? (UInt32(unit) | 0x80000000)
        var modifiers: UInt32 = 0
        for (flag, bit): (NSEvent.ModifierFlags, UInt32) in [(.command, 0x100), (.shift, 0x200), (.capsLock, 0x400), (.option, 0x800), (.control, 0x1000)] {
            if e.modifierFlags.contains(flag) { modifiers |= bit }
        }
        var op = event(10, nil), value = op["event"] as! JSON
        value["key_code"] = code; value["modifiers"] = modifiers; op["event"] = value; send?(op)
    }
}

final class App: NSObject, NSApplicationDelegate, NSWindowDelegate {
    let session: Session, template: JSON, exercise: String?
    let queue = DispatchQueue(label: "aex.custom-ui.worker")
    var window: NSWindow!, canvas: Canvas!, label: NSTextField!
    let rasterizer = Rasterizer()
    var pending: [JSON] = [], busy = false, closing = false, failed = false
    var steps: [(NSEvent.EventType, CGFloat, CGFloat)] = []
    var snapshots = 0
    var signals: [DispatchSourceSignal] = []
    init(session: Session, template: JSON, exercise: String?) { self.session = session; self.template = template; self.exercise = exercise }
    func applicationDidFinishLaunching(_ notification: Notification) {
        let f = template["frame"] as! [Int], w = f[2]-f[0], h = f[3]-f[1]
        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: max(w, 420), height: h+65), styleMask: [.titled, .closable, .miniaturizable], backing: .buffered, defer: false)
        window.title = "AEXCompat — Live custom UI"; window.delegate = self; window.isReleasedWhenClosed = false
        canvas = Canvas(frame: NSRect(x: 0, y: 65, width: w, height: h)); canvas.eventTemplate = template
        canvas.image = NSImage(size: canvas.bounds.size); canvas.send = { [weak self] op in self?.enqueue(op) }
        window.contentView!.addSubview(canvas)
        label = NSTextField(wrappingLabelWithString: "Starting guest…"); label.frame = NSRect(x: 8, y: 5, width: max(w,420)-16, height: 54); window.contentView!.addSubview(label)
        window.center(); window.makeKeyAndOrderFront(nil); window.makeFirstResponder(canvas)
        NSApp.activate(ignoringOtherApps: true)
        if exercise != nil {
            // These are real NSEvents delivered through NSWindow.sendEvent, not
            // recorded worker requests. Same responder and coordinate path as a mouse.
            switch exercise! {
            case "insert": steps = [(.leftMouseDown, 100, 30), (.leftMouseDragged, 130, 30), (.leftMouseUp, 130, 30), (.keyDown, 130, 30)]
            case "delete": steps = [(.leftMouseDown, 10, 65), (.leftMouseDragged, 50, 76), (.leftMouseUp, 50, 76), (.keyDown, 50, 76)]
            default: steps = [(.leftMouseDown, 260, 15), (.leftMouseDragged, 260, 150), (.leftMouseUp, 260, 150), (.leftMouseDown, 260, 38), (.leftMouseUp, 260, 38), (.keyDown, 260, 38)]
            }
        }
        for sig in [SIGINT, SIGTERM, SIGHUP] {
            signal(sig, SIG_IGN)
            let source = DispatchSource.makeSignalSource(signal: sig, queue: .main)
            source.setEventHandler { [weak self] in self?.finish() }
            source.resume(); signals.append(source)
        }
        canvas.needsDisplay = true
    }
    func enqueue(_ op: JSON) {
        guard !closing && !failed else { return }
        // Coalesce only consecutive pending non-final drag moves. Never drop a
        // click, mouse-up, key, or the movement currently executing in the guest.
        if let e = op["event"] as? JSON, number(e["event"]) == 3, e["last"] as? Bool == false,
           let previous = pending.last?["event"] as? JSON, number(previous["event"]) == 3, previous["last"] as? Bool == false {
            pending[pending.count - 1] = op; return
        }
        guard pending.count < 128 else { fail(CanvasError("Input queue overflow")); return }
        pending.append(op); pump()
    }
    func pump() {
        guard !busy && !pending.isEmpty && !closing && !failed else { return }
        if session.count >= 256 { pending.removeAll(); finish(); return }
        busy = true
        let op = pending.removeFirst()
        if let e = op["event"] as? JSON, number(e["event"]) == 3 && !canvas.activeDrag {
            busy = false; pump(); return
        }
        queue.async { [self] in
            let result = Result { try session.exchange(op) }
            DispatchQueue.main.async { [self] in
                busy = false
                do {
                    let row = try result.get(), report = row["event"] as! JSON
                    let event = op["event"] as! JSON, kind = number(event["event"])
                    if kind == 2 { canvas.activeDrag = number(report["send_drag"]) != 0 }
                    if kind == 3 && event["last"] as? Bool == true { canvas.activeDrag = false }
                    // The whole guest frame uses window coordinates; canvas origin is frame.left/top.
                    let f = template["frame"] as! [Int]
                    let origin = NSPoint(x: f[0], y: f[1])
                    if kind == 4 {
                        let frame = NSImage(size: canvas.bounds.size)
                        try rasterizer.render(report["calls"] as? [JSON] ?? [], into: frame, origin: origin)
                        canvas.image = frame; canvas.needsDisplay = true; canvas.ready = true
                        try snapshot()
                    } else {
                        try rasterizer.render(report["calls"] as? [JSON] ?? [], into: canvas.image, origin: origin)
                        pending.insert(canvas.event(4, nil), at: 0)
                    }
                    label.stringValue = "\(session.count)/256 operations · slot \(number(template["slot"]))\nClick / drag · ASCII keys · close to save. AppKit font preview."
                    if pending.isEmpty && exercise != nil { injectNext() } else { pump() }
                } catch { fail(error) }
            }
        }
    }
    func snapshot() throws {
        // Retain first and most recent actual canvas raster without accumulating frames.
        guard let tiff = canvas.image.tiffRepresentation, let bitmap = NSBitmapImageRep(data: tiff), let png = bitmap.representation(using: .png, properties: [:]) else { throw CanvasError("Cannot capture canvas") }
        try png.write(to: session.directory.appendingPathComponent(snapshots == 0 ? "first.png" : "last.png"))
        snapshots += 1
    }
    func injectNext() {
        if steps.isEmpty { finish(); return }
        let (type,x,y) = steps.removeFirst()
        let point = canvas.convert(NSPoint(x:x,y:y), to:nil)
        let e: NSEvent?
        if type == .keyDown {
            e = NSEvent.keyEvent(with: .keyDown, location: point, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber, context: nil, characters: "a", charactersIgnoringModifiers: "a", isARepeat: false, keyCode: 0)
        } else {
            e = NSEvent.mouseEvent(with:type, location:point, modifierFlags:[], timestamp:ProcessInfo.processInfo.systemUptime, windowNumber:window.windowNumber, context:nil, eventNumber:0, clickCount:1, pressure:1)
        }
        guard let event = e else { fail(CanvasError("Could not create NSEvent")); return }
        window.sendEvent(event)
        if !busy && pending.isEmpty { fail(CanvasError("Injected event was not dispatched")) }
    }
    func fail(_ error: Error) {
        failed = true; label.stringValue = "ERROR: \(error)\nClose to retain diagnostics."
        fputs("canvas_error: \(error)\n", stderr)
        if session.process.isRunning { kill(-session.process.processIdentifier, SIGKILL); kill(session.process.processIdentifier, SIGKILL) }
        if exercise != nil { finish() }
    }
    func windowShouldClose(_ sender: NSWindow) -> Bool { finish(); return false }
    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply { finish(); return .terminateLater }
    func finish() {
        guard !closing else { return }; closing = true; canvas.ready = false
        queue.async { [self] in
            var ok = !failed
            do { try session.close() } catch { fputs("canvas_close: \(error)\n", stderr); ok = false }
            DispatchQueue.main.async { exit(ok ? 0 : 1) }
        }
    }
}

// Positional CLI keeps plugin/config paths out of saved, shareable provenance.
let args = CommandLine.arguments
if args.count == 5 && args[1] == "--worker-child" {
    // Keep resource limits in the child, never constrain the AppKit process.
    guard setpgid(0, 0) == 0 || getpgrp() == getpid() else { exit(126) }
    for (resource, value): (Int32, rlim_t) in [(RLIMIT_CPU, 120), (RLIMIT_NOFILE, 64), (RLIMIT_FSIZE, 64 << 20), (RLIMIT_CORE, 0)] {
        var limit = rlimit(rlim_cur: value, rlim_max: value)
        guard setrlimit(resource, &limit) == 0 else { exit(126) }
    }
    let arguments: [String] = [args[2], "ui-session", args[3], args[4], "--stream"]
    let strings = arguments.map { arg in arg.withCString { strdup($0) } }
    var pointers = strings + [nil]
    execv(args[2], &pointers)
    exit(127)
}
if args.count < 5 || args.count > 6 {
    fputs("usage: mac-custom-ui WORKER PLUGIN CONFIG.json NEW_OUTPUT_DIR [insert|delete|presets]\n", stderr); exit(2)
}
do {
    let config = try dictionary(Data(contentsOf: URL(fileURLWithPath: args[3])))
    guard let header = config["session"] as? JSON, let template = config["event"] as? JSON,
          let frame = template["frame"] as? [Int], frame.count == 4,
          frame[0] >= 0 && frame[1] >= 0 && frame[2] > frame[0] && frame[3] > frame[1] && frame[2] <= 2048 && frame[3] <= 2048,
          args.count == 5 || ["insert", "delete", "presets"].contains(args[5]) else { throw CanvasError("Invalid canvas configuration/exercise") }
    signal(SIGPIPE, SIG_IGN)
    let session = try Session(worker: URL(fileURLWithPath: args[1]), plugin: URL(fileURLWithPath: args[2]), directory: URL(fileURLWithPath: args[4]), request: header)
    let app = NSApplication.shared
    let delegate = App(session: session, template: template, exercise: args.count == 6 ? args[5] : nil)
    app.setActivationPolicy(.regular); app.delegate = delegate; app.run()
} catch { fputs("canvas_error: \(error)\n", stderr); exit(1) }
