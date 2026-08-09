// RoNtoyOverlay — a topmost, click-through second-screen card for the live RoNtoy feed.
//
// Read-only by construction. The only I/O this process performs against anything
// game-related is an outbound HTTP GET to the loopback RoNtoy host. It does not open
// the game process, read or write its memory, inject anything, synthesize input, or
// install a hook. It is a separate macOS window compositing above the Parallels
// window; it touches the guest not at all.
//
// Two deliberate limits, stated plainly:
//   * `ignoresMouseEvents = true` means it can never be clicked, dragged, or focused.
//     Every setting is a launch flag.
//   * A window at any level still cannot draw over a *fullscreen-exclusive* Direct3D
//     surface. On this host that is moot: the game runs inside a Parallels window,
//     and this overlay composites above that window like any other Mac window.
//
// Build:  swiftc -O tools/rontoy-overlay/RoNtoyOverlay.swift -o <out>/rontoy-overlay
// Run:    rontoy-overlay --port 17360 --follow-parallels

import AppKit
import Foundation

// MARK: - configuration

struct Config {
    var port: Int = 17360
    var host: String = "127.0.0.1"
    var corner: String = "tr"
    var margin: CGFloat = 18
    var width: CGFloat = 360
    var opacity: CGFloat = 0.92
    var pollSeconds: Double = 0.5
    var runSeconds: Double? = nil
    var snapshotPath: String? = nil
    var followParallels: Bool = true
    var anchorOwner: String = "Parallels"

    static func parse(_ argv: [String]) -> Config {
        var config = Config()
        var index = 0
        func next(_ flag: String) -> String {
            index += 1
            guard index < argv.count else {
                FileHandle.standardError.write("rontoy-overlay: \(flag) needs a value\n".data(using: .utf8)!)
                exit(2)
            }
            return argv[index]
        }
        while index < argv.count {
            switch argv[index] {
            case "--port": config.port = Int(next("--port")) ?? config.port
            case "--host": config.host = next("--host")
            case "--corner": config.corner = next("--corner")
            case "--margin": config.margin = CGFloat(Double(next("--margin")) ?? 18)
            case "--width": config.width = CGFloat(Double(next("--width")) ?? 360)
            case "--opacity": config.opacity = CGFloat(Double(next("--opacity")) ?? 0.92)
            case "--poll": config.pollSeconds = Double(next("--poll")) ?? 0.5
            case "--seconds": config.runSeconds = Double(next("--seconds"))
            case "--snapshot": config.snapshotPath = next("--snapshot")
            case "--follow-parallels": config.followParallels = true
            case "--no-follow": config.followParallels = false
            case "--anchor-owner": config.anchorOwner = next("--anchor-owner")
            case "--dump-windows":
                dumpWindows()
                exit(0)
            case "-h", "--help":
                print(usage)
                exit(0)
            default:
                FileHandle.standardError.write("rontoy-overlay: unknown flag \(argv[index])\n".data(using: .utf8)!)
                exit(2)
            }
            index += 1
        }
        guard (1...65535).contains(config.port) else { fail("--port must be in 1...65535") }
        guard ["127.0.0.1", "localhost"].contains(config.host.lowercased()) else {
            fail("--host must remain loopback (127.0.0.1 or localhost)")
        }
        guard ["tl", "tr", "bl", "br"].contains(config.corner) else {
            fail("--corner must be tl, tr, bl, or br")
        }
        guard config.margin >= 0, config.width >= 240, config.width <= 1_200 else {
            fail("--margin must be non-negative and --width must be in 240...1200")
        }
        guard config.opacity > 0, config.opacity <= 1 else { fail("--opacity must be in (0, 1]") }
        guard config.pollSeconds >= 0.1, config.pollSeconds <= 10 else { fail("--poll must be in 0.1...10") }
        if let seconds = config.runSeconds, seconds <= 0 { fail("--seconds must be positive") }
        return config
    }

    private static func fail(_ message: String) -> Never {
        FileHandle.standardError.write("rontoy-overlay: \(message)\n".data(using: .utf8)!)
        exit(2)
    }
}

let usage = """
rontoy-overlay - topmost click-through card for the loopback RoNtoy host

  --port <n>            host port (default 17360)
  --corner <tl|tr|bl|br>  anchor corner of the target window (default tr)
  --margin <px>         inset from that corner (default 18)
  --width <px>          card width (default 360)
  --opacity <0..1>      card opacity (default 0.92)
  --poll <seconds>      host poll interval (default 0.5)
  --seconds <n>         exit after n seconds
  --snapshot <path>     also render one PNG of the card to this path
  --follow-parallels    anchor to the Parallels window (default)
  --no-follow           anchor to the main screen instead
  --anchor-owner <s>    owner-name substring to anchor to (default "Parallels")
  --dump-windows        list on-screen window owners and bounds, then exit

The overlay is read-only: it reads the loopback host over HTTP and nothing else.
It ignores all mouse events, so it can never be clicked or take focus.
"""

func dumpWindows() {
    let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] ?? []
    for window in list {
        let owner = window[kCGWindowOwnerName as String] as? String ?? "?"
        let layer = window[kCGWindowLayer as String] as? Int ?? -1
        let bounds = window[kCGWindowBounds as String] as? [String: CGFloat] ?? [:]
        print("\(owner)\tlayer=\(layer)\t\(bounds["X"] ?? 0),\(bounds["Y"] ?? 0) \(bounds["Width"] ?? 0)x\(bounds["Height"] ?? 0)")
    }
}

// MARK: - host feed (the only network I/O in the process)

struct Feed {
    var advice: [(severity: String, title: String, detail: String, action: String)] = []
    var resources: [(name: String, stock: Double, rate: Double, clamped: Int)] = []
    var populationUsed = 0
    var populationCap = 0
    var idleCitizens: Int? = nil
    var idleFishermen: Int? = nil
    var gatherers: Int? = nil
    var frame = 0
    var sequence = 0
    var gameSeconds = 0
    var rateAgeFrames = 0
    var ageMs = 0
    var adviceAllowed = true
    var suppressed: [String] = []
    var paused: Bool? = nil
    /// `fresh`, `stale`, `waiting`, or `unavailable`, proven by `/v1/status`.
    var sourceStatus = "waiting"
    var sessionId = ""
    var error: String? = "waiting for the RoNtoy host"
}

let resourceOrder = ["food", "timber", "wealth", "knowledge", "metal", "oil"]

final class FeedReader {
    private let url: URL
    private let statusURL: URL
    private let session: URLSession
    private var feed = Feed()
    /// Kept outside `feed` so a `/v1/latest` reply cannot clobber the freshness a
    /// concurrent `/v1/status` reply just measured.
    private var ageMs = 0
    private var sourceStatus = "waiting"
    private let lock = NSLock()

    init(config: Config) {
        url = URL(string: "http://\(config.host):\(config.port)/v1/latest")!
        statusURL = URL(string: "http://\(config.host):\(config.port)/v1/status")!
        let configuration = URLSessionConfiguration.ephemeral
        configuration.timeoutIntervalForRequest = 2
        configuration.httpShouldSetCookies = false
        session = URLSession(configuration: configuration)
    }

    func snapshot() -> Feed {
        lock.lock(); defer { lock.unlock() }
        var current = feed
        current.ageMs = ageMs
        current.sourceStatus = sourceStatus
        // `/v1/latest` retains one immutable observation. It is useful for display,
        // but never sufficient freshness evidence by itself. Only a successful
        // monotonic `/v1/status` reading can make advice visible.
        if current.error != nil || sourceStatus != "fresh" {
            current.adviceAllowed = false
            let reason = sourceStatus == "stale" ? "source_stale"
                : sourceStatus == "waiting" ? "source_waiting" : "source_unavailable"
            if !current.suppressed.contains(reason) { current.suppressed.append(reason) }
        }
        return current
    }

    func poll(_ done: @escaping () -> Void) {
        session.dataTask(with: url) { [weak self] data, response, error in
            guard let self else { return }
            var next = Feed()
            if let error {
                next.error = "host unreachable: \(error.localizedDescription)"
            } else if let http = response as? HTTPURLResponse, http.statusCode != 200 {
                next.error = http.statusCode == 404 ? "host up, no snapshot admitted yet" : "host HTTP \(http.statusCode)"
            } else if let data, let root = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] {
                next = Self.decode(root)
            } else {
                next.error = "host returned an unreadable body"
            }
            self.lock.lock(); self.feed = next; self.lock.unlock()
            done()
        }.resume()
        session.dataTask(with: statusURL) { [weak self] data, response, error in
            guard let self else { return }
            var nextStatus = "unavailable"
            var nextAge = self.ageMs
            if error == nil, let http = response as? HTTPURLResponse, http.statusCode == 200,
               let data,
               let root = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] {
                if root["latest"] is NSNull {
                    nextStatus = "waiting"
                    nextAge = 0
                } else if let latest = root["latest"] as? [String: Any],
                          let age = latest["age_ms"] as? Int,
                          let stale = latest["stale"] as? Bool {
                    nextAge = max(0, age)
                    nextStatus = stale ? "stale" : "fresh"
                }
            }
            self.lock.lock()
            self.ageMs = nextAge
            self.sourceStatus = nextStatus
            self.lock.unlock()
            done()
        }.resume()
    }

    static func decode(_ root: [String: Any]) -> Feed {
        var feed = Feed()
        feed.error = nil
        guard let snapshot = root["snapshot"] as? [String: Any],
              let analysis = root["analysis"] as? [String: Any],
              let economy = snapshot["economy"] as? [String: Any] else {
            feed.error = "host envelope was not the expected shape"
            return feed
        }
        let game = snapshot["game"] as? [String: Any] ?? [:]
        feed.frame = game["frame"] as? Int ?? 0
        feed.gameSeconds = game["seconds"] as? Int ?? 0
        feed.paused = game["paused"] as? Bool
        let source = snapshot["source"] as? [String: Any] ?? [:]
        feed.sequence = source["sequence"] as? Int ?? 0
        feed.sessionId = source["session_id"] as? String ?? ""
        let rate = economy["rate_sample"] as? [String: Any] ?? [:]
        feed.rateAgeFrames = rate["age_frames"] as? Int ?? 0

        let resources = economy["resources"] as? [String: Any] ?? [:]
        let clamp = ((economy["clamp"] as? [String: Any])?["resources"] as? [String: Any]) ?? [:]
        for name in resourceOrder {
            guard let entry = resources[name] as? [String: Any] else { continue }
            let stock = (entry["stock"] as? NSNumber)?.doubleValue ?? 0
            let income = (entry["income_per_min"] as? NSNumber)?.doubleValue ?? 0
            let over = ((clamp[name] as? [String: Any])?["over_cap"] as? NSNumber)?.intValue ?? 0
            feed.resources.append((name, stock, income, over))
        }
        let population = economy["population"] as? [String: Any] ?? [:]
        feed.populationUsed = population["used"] as? Int ?? 0
        feed.populationCap = population["cap"] as? Int ?? 0
        feed.idleCitizens = population["idle_citizens"] as? Int
        let workers = economy["workers"] as? [String: Any] ?? [:]
        feed.idleFishermen = workers["idle_fishermen"] as? Int
        feed.gatherers = workers["gatherers"] as? Int

        feed.adviceAllowed = analysis["advice_allowed"] as? Bool ?? true
        feed.suppressed = analysis["suppressed_reasons"] as? [String] ?? []
        for item in analysis["advice"] as? [[String: Any]] ?? [] {
            feed.advice.append((
                item["severity"] as? String ?? "info",
                item["title"] as? String ?? "",
                item["detail"] as? String ?? "",
                item["action"] as? String ?? ""
            ))
        }
        return feed
    }
}

// MARK: - drawing

let inkPrimary = NSColor(white: 0.96, alpha: 1)
let inkMuted = NSColor(white: 0.62, alpha: 1)
let accentTeal = NSColor(red: 0.33, green: 0.84, blue: 0.82, alpha: 1)
let accentAmber = NSColor(red: 1.0, green: 0.74, blue: 0.41, alpha: 1)
let accentRed = NSColor(red: 1.0, green: 0.47, blue: 0.43, alpha: 1)

func severityColor(_ severity: String) -> NSColor {
    switch severity {
    case "critical": return accentRed
    case "warning": return accentAmber
    default: return accentTeal
    }
}

final class CardView: NSView {
    var feed = Feed()
    private let pad: CGFloat = 14

    override var isFlipped: Bool { true }

    private func draw(_ string: String, at point: NSPoint, size: CGFloat, color: NSColor,
                      weight: NSFont.Weight = .regular, mono: Bool = false, width: CGFloat? = nil) -> CGFloat {
        let font = mono
            ? NSFont.monospacedSystemFont(ofSize: size, weight: weight)
            : NSFont.systemFont(ofSize: size, weight: weight)
        let attributes: [NSAttributedString.Key: Any] = [.font: font, .foregroundColor: color]
        let text = NSAttributedString(string: string, attributes: attributes)
        if let width {
            let box = NSRect(x: point.x, y: point.y, width: width, height: 400)
            let bounds = text.boundingRect(with: NSSize(width: width, height: 400),
                                           options: [.usesLineFragmentOrigin, .usesFontLeading])
            text.draw(with: box, options: [.usesLineFragmentOrigin, .usesFontLeading])
            return ceil(bounds.height)
        }
        text.draw(at: point)
        return ceil(text.size().height)
    }

    /// Height the card needs for the current feed, so the window can be sized before drawing.
    func requiredHeight() -> CGFloat {
        var height = pad + 20 + 8            // header
        height += 2 * 34 + 6                 // two rows of resource chips
        height += 26                         // population line
        height += 22                         // counters line
        if feed.error != nil { height += 22 }
        for item in visibleAdvice() {
            height += 20
            height += ceil(measure(item.detail, size: 11, width: bounds.width - 2 * pad - 10))
            height += ceil(measure(item.action, size: 11, width: bounds.width - 2 * pad - 10))
            height += 12
        }
        if visibleAdvice().isEmpty { height += 20 }
        return height + pad
    }

    private func measure(_ string: String, size: CGFloat, width: CGFloat) -> CGFloat {
        let text = NSAttributedString(string: string, attributes: [.font: NSFont.systemFont(ofSize: size)])
        return text.boundingRect(with: NSSize(width: max(40, width), height: 400),
                                 options: [.usesLineFragmentOrigin, .usesFontLeading]).height
    }

    private func visibleAdvice() -> [(severity: String, title: String, detail: String, action: String)] {
        feed.adviceAllowed ? Array(feed.advice.prefix(3)) : []
    }

    override func draw(_ dirtyRect: NSRect) {
        let card = NSBezierPath(roundedRect: bounds, xRadius: 14, yRadius: 14)
        NSColor(red: 0.043, green: 0.063, blue: 0.078, alpha: 0.93).setFill()
        card.fill()
        NSColor(white: 1, alpha: 0.10).setStroke()
        card.lineWidth = 1
        card.stroke()

        let inner = bounds.width - 2 * pad
        var y = pad

        _ = draw("RONTOY", at: NSPoint(x: pad, y: y), size: 12, color: accentTeal, weight: .bold)
        let stateLabel: String
        if feed.paused == true { stateLabel = "PAUSED"
        } else if feed.sourceStatus == "stale" { stateLabel = "STALE"
        } else if feed.sourceStatus == "unavailable" { stateLabel = "SOURCE LOST"
        } else if feed.sourceStatus == "waiting" { stateLabel = "WAITING"
        } else { stateLabel = "\(feed.ageMs) ms · f\(feed.frame)" }
        let freshColor = feed.sourceStatus == "fresh" ? (feed.paused == true ? accentAmber : inkMuted)
            : (feed.sourceStatus == "waiting" ? accentAmber : accentRed)
        let clock = String(format: "%d:%02d", feed.gameSeconds / 60, feed.gameSeconds % 60)
        let right = NSAttributedString(string: "\(clock)  \(stateLabel)", attributes: [
            .font: NSFont.monospacedSystemFont(ofSize: 10, weight: .regular), .foregroundColor: freshColor,
        ])
        right.draw(at: NSPoint(x: bounds.width - pad - right.size().width, y: y + 1))
        y += 22

        if let error = feed.error {
            _ = draw(error, at: NSPoint(x: pad, y: y), size: 11, color: accentRed, width: inner)
            y += 22
        }

        // Resource chips: two rows of three, retail order, clamp marked with a caret.
        let chipWidth = inner / 3
        for (index, resource) in feed.resources.enumerated() {
            let column = CGFloat(index % 3), row = CGFloat(index / 3)
            let x = pad + column * chipWidth
            let top = y + row * 34
            let clamped = resource.clamped > 0
            _ = draw(resource.name.uppercased(), at: NSPoint(x: x, y: top), size: 8,
                     color: clamped ? accentAmber : inkMuted, weight: .semibold, mono: true)
            _ = draw(String(format: "%.0f", resource.stock), at: NSPoint(x: x, y: top + 10), size: 14,
                     color: inkPrimary, weight: .medium, mono: true)
            let rate = String(format: "%@%.0f/m%@", resource.rate >= 0 ? "+" : "", resource.rate, clamped ? " ^" : "")
            _ = draw(rate, at: NSPoint(x: x + 52, y: top + 14), size: 9,
                     color: clamped ? accentAmber : accentTeal, mono: true)
        }
        y += 2 * 34 + 6

        // Population bar.
        let used = CGFloat(feed.populationUsed), cap = CGFloat(max(feed.populationCap, 1))
        let barWidth = inner - 96
        let track = NSBezierPath(roundedRect: NSRect(x: pad + 96, y: y + 6, width: barWidth, height: 6),
                                 xRadius: 3, yRadius: 3)
        NSColor(white: 1, alpha: 0.12).setFill(); track.fill()
        let ratio = min(1, used / cap)
        let fillColor = ratio >= 1 ? accentRed : (ratio > 0.9 ? accentAmber : accentTeal)
        let fill = NSBezierPath(roundedRect: NSRect(x: pad + 96, y: y + 6, width: barWidth * ratio, height: 6),
                                xRadius: 3, yRadius: 3)
        fillColor.setFill(); fill.fill()
        _ = draw("POP \(feed.populationUsed)/\(feed.populationCap)", at: NSPoint(x: pad, y: y), size: 11,
                 color: inkPrimary, weight: .medium, mono: true)
        y += 26

        var counters: [String] = []
        if let idle = feed.idleCitizens { counters.append("idle \(idle)") }
        if let fishers = feed.idleFishermen { counters.append("idle fish \(fishers)") }
        if let gatherers = feed.gatherers { counters.append("gatherers \(gatherers)") }
        counters.append("rate age \(feed.rateAgeFrames)f")
        _ = draw(counters.joined(separator: " · "), at: NSPoint(x: pad, y: y), size: 10, color: inkMuted, mono: true)
        y += 22

        let items = visibleAdvice()
        if items.isEmpty {
            let message: String
            if feed.paused == true { message = "advice paused with the game"
            } else if feed.adviceAllowed { message = "no advice from this capture"
            } else { message = "advice suppressed: \(feed.suppressed.joined(separator: ", "))" }
            _ = draw(message, at: NSPoint(x: pad, y: y), size: 11, color: inkMuted, width: inner)
        }
        for item in items {
            let color = severityColor(item.severity)
            let stripe = NSBezierPath(roundedRect: NSRect(x: pad, y: y + 2, width: 3, height: 12),
                                      xRadius: 1.5, yRadius: 1.5)
            color.setFill(); stripe.fill()
            _ = draw(item.title, at: NSPoint(x: pad + 10, y: y), size: 12, color: color, weight: .semibold)
            y += 20
            y += draw(item.detail, at: NSPoint(x: pad + 10, y: y), size: 11, color: inkPrimary,
                      width: inner - 10)
            y += draw(item.action, at: NSPoint(x: pad + 10, y: y), size: 11, color: inkMuted,
                      width: inner - 10)
            y += 12
        }
    }
}

// MARK: - window placement

func anchorRect(config: Config) -> NSRect {
    let screen = NSScreen.screens.first ?? NSScreen.main!
    guard config.followParallels else { return screen.visibleFrame }
    let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] ?? []
    for window in list {
        guard let owner = window[kCGWindowOwnerName as String] as? String,
              owner.localizedCaseInsensitiveContains(config.anchorOwner),
              (window[kCGWindowLayer as String] as? Int) == 0,
              let bounds = window[kCGWindowBounds as String] as? [String: CGFloat],
              let x = bounds["X"], let y = bounds["Y"], let w = bounds["Width"], let h = bounds["Height"],
              w > 400, h > 300 else { continue }
        // CGWindow bounds are top-left origin on the primary display; Cocoa is bottom-left.
        return NSRect(x: x, y: screen.frame.maxY - (y + h), width: w, height: h)
    }
    return screen.visibleFrame
}

func placement(card: NSSize, in target: NSRect, config: Config) -> NSPoint {
    let margin = config.margin
    switch config.corner {
    case "tl": return NSPoint(x: target.minX + margin, y: target.maxY - card.height - margin)
    case "bl": return NSPoint(x: target.minX + margin, y: target.minY + margin)
    case "br": return NSPoint(x: target.maxX - card.width - margin, y: target.minY + margin)
    default: return NSPoint(x: target.maxX - card.width - margin, y: target.maxY - card.height - margin)
    }
}

// MARK: - application

final class OverlayApp: NSObject, NSApplicationDelegate {
    let config: Config
    let reader: FeedReader
    let window: NSWindow
    let view: CardView
    private var timer: Timer?
    private var wroteSnapshot = false

    init(config: Config) {
        self.config = config
        reader = FeedReader(config: config)
        view = CardView(frame: NSRect(x: 0, y: 0, width: config.width, height: 240))
        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: config.width, height: 240),
                          styleMask: [.borderless], backing: .buffered, defer: false)
        super.init()
        window.isOpaque = false
        window.backgroundColor = .clear
        window.hasShadow = true
        window.alphaValue = config.opacity
        // Above normal and floating windows, including a windowed VM. A window at any
        // level cannot cover a fullscreen-exclusive D3D surface; that limit is the
        // product's, not this level's.
        window.level = NSWindow.Level(rawValue: Int(CGWindowLevelForKey(.screenSaverWindow)))
        window.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary, .ignoresCycle]
        // The whole read-only promise in one line: the overlay can never be clicked,
        // dragged, focused, or used to send anything anywhere.
        window.ignoresMouseEvents = true
        window.contentView = view
        window.orderFrontRegardless()
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        refresh()
        timer = Timer.scheduledTimer(withTimeInterval: config.pollSeconds, repeats: true) { [weak self] _ in
            self?.refresh()
        }
        if let seconds = config.runSeconds {
            Timer.scheduledTimer(withTimeInterval: seconds, repeats: false) { _ in NSApp.terminate(nil) }
        }
    }

    func refresh() {
        reader.poll { [weak self] in
            DispatchQueue.main.async { self?.render() }
        }
    }

    func render() {
        view.feed = reader.snapshot()
        let height = view.requiredHeight()
        let target = anchorRect(config: config)
        let size = NSSize(width: config.width, height: height)
        window.setFrame(NSRect(origin: placement(card: size, in: target, config: config), size: size), display: true)
        view.frame = NSRect(origin: .zero, size: size)
        view.needsDisplay = true
        view.displayIfNeeded()
        if let path = config.snapshotPath, !wroteSnapshot, view.feed.error == nil {
            wroteSnapshot = true
            writeSnapshot(to: path)
        }
    }

    /// Render the card itself to a PNG. This captures our own view, never the screen,
    /// so it needs no screen-recording permission and can see nothing but our card.
    func writeSnapshot(to path: String) {
        guard let rep = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { return }
        view.cacheDisplay(in: view.bounds, to: rep)
        guard let data = rep.representation(using: .png, properties: [:]) else { return }
        try? data.write(to: URL(fileURLWithPath: path))
        FileHandle.standardError.write("rontoy-overlay: wrote \(path)\n".data(using: .utf8)!)
    }
}

let application = NSApplication.shared
let configuration = Config.parse(Array(CommandLine.arguments.dropFirst()))
// .accessory: no Dock tile, no menu bar, never becomes the active application.
application.setActivationPolicy(.accessory)
let delegate = OverlayApp(config: configuration)
application.delegate = delegate
application.run()
