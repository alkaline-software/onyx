// A finger on Onyx in a simulator, through UIKit as a real one goes:
// the steps in the file TOUCH_STEPS names, one a line, in points from the
// screen's top left. Driven by scripts/touch.R (docs/probing.md).
import XCTest

final class Touch: XCTestCase {
    func testSteps() throws {
        let env = ProcessInfo.processInfo.environment
        let path = env["TOUCH_STEPS"] ?? "/tmp/touch-steps.txt"
        let steps = try String(contentsOfFile: path, encoding: .utf8).split(separator: "\n").map(String.init)
        let app = XCUIApplication(bundleIdentifier: env["TOUCH_APP"] ?? "com.alkalinesoftware.onyx.dev")
        var launch: [String: String] = [:]
        for (k, v) in env where k.hasPrefix("TOUCH_ONYX_") { launch[String(k.dropFirst(6))] = v }
        app.launchEnvironment = launch
        app.launch()
        let screen = app.windows.firstMatch.frame.size
        func at(_ x: Double, _ y: Double) -> XCUICoordinate {
            app.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: x, dy: y))
        }
        _ = screen
        for line in steps {
            let parts = line.split(separator: " ", maxSplits: 1).map(String.init)
            guard let verb = parts.first else { continue }
            let rest = parts.count > 1 ? parts[1] : ""
            let n = rest.split(separator: ",").compactMap { Double($0.trimmingCharacters(in: .whitespaces)) }
            print("touch step \(line)")
            switch verb {
            case "wait": Thread.sleep(forTimeInterval: (n.first ?? 500) / 1000)
            case "tap": at(n[0], n[1]).tap()
            case "double": at(n[0], n[1]).doubleTap()
            case "hold": at(n[0], n[1]).press(forDuration: n.count > 2 ? n[2] / 1000 : 0.8)
            case "drag":
                // down at the first place, held so long, moved to the second, and lifted
                at(n[0], n[1]).press(forDuration: n.count > 4 ? n[4] / 1000 : 0.05, thenDragTo: at(n[2], n[3]))
            case "holddrag":
                at(n[0], n[1]).press(forDuration: n.count > 4 ? n[4] / 1000 : 0.8, thenDragTo: at(n[2], n[3]), withVelocity: .slow, thenHoldForDuration: 0.3)
            case "type": app.typeText(rest)
            case "keys":
                // each letter pressed on the system keyboard, as a finger types: \n is Return
                for c in rest.replacingOccurrences(of: "\\n", with: "\n") {
                    let k = app.keyboards.firstMatch
                    let name = c == "\n" ? "Return" : c == " " ? "space" : String(c)
                    let tries = name == "Return" ? ["Return", "return", "Done", "done", "Go", "go", "Search", "search"] : [name, name.lowercased(), name.uppercased()]
                    if let hit = tries.map({ k.keys[$0] }).first(where: { $0.exists }) ?? tries.map({ k.buttons[$0] }).first(where: { $0.exists }) { hit.tap() } else { print("touch nokey \(name)") }
                }
            case "shot":
                let png = XCUIScreen.main.screenshot().pngRepresentation
                try png.write(to: URL(fileURLWithPath: rest))
            case "menu":
                // the system's edit menu's items, as accessibility reads them
                let items = app.menuItems.allElementsBoundByIndex.map { $0.label }
                print("touch menu \(items)")
            case "pick":
                app.menuItems[rest].firstMatch.tap()
            default: print("touch unknown \(verb)")
            }
        }
    }
}
