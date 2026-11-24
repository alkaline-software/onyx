import Cocoa
import WebKit
// render.swift <icon.html> <out.png>:<size>:<margin|bleed|opaque|template> ...
// Loads the canvas source in a web view with no window and writes what its
// exportIcon(size, bleed) returns, once per job, or for `template` what
// exportTemplate(size) returns (the menu bar's image). `opaque` is `bleed`
// written with no alpha channel, as an iPhone's icon must be. The canvas draws each
// size on its own pixel grid, so nothing here is a scaled-down copy of another.
let args = CommandLine.arguments
let page = URL(fileURLWithPath: args[1])
var jobs = args.dropFirst(2).map { $0.split(separator: ":").map(String.init) }

final class Renderer: NSObject, WKNavigationDelegate {
    let view = WKWebView(frame: NSRect(x: 0, y: 0, width: 1280, height: 800))
    func start() {
        view.navigationDelegate = self
        view.loadFileURL(page, allowingReadAccessTo: page.deletingLastPathComponent())
    }
    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) { next() }
    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) { fail("\(error)") }
    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) { fail("\(error)") }
    func fail(_ why: String) -> Never {
        FileHandle.standardError.write("render.swift: \(why)\n".data(using: .utf8)!)
        exit(1)
    }
    // The same pixels with no alpha channel: drawn again on a ground that has none.
    func opaque(_ png: Data, _ out: String) -> Data {
        guard let image = NSBitmapImageRep(data: png)?.cgImage,
              let ctx = CGContext(data: nil, width: image.width, height: image.height, bitsPerComponent: 8, bytesPerRow: 0, space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)
        else { fail("\(out): no picture to make opaque") }
        ctx.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
        guard let flat = ctx.makeImage(), let data = NSBitmapImageRep(cgImage: flat).representation(using: .png, properties: [:]) else { fail("\(out): not made opaque") }
        return data
    }
    func next() {
        guard let job = jobs.first else { exit(0) }
        jobs.removeFirst()
        guard job.count == 3 else { fail("a job is <out.png>:<size>:<margin|bleed|opaque|template>, not \(job.joined(separator: ":"))") }
        let (out, size, kind) = (job[0], job[1], job[2])
        let call = kind == "template" ? "exportTemplate(\(size))" : "exportIcon(\(size), \(kind != "margin"))"
        view.evaluateJavaScript(call) { result, error in
            guard let url = result as? String, let comma = url.firstIndex(of: ","),
                  let png = Data(base64Encoded: String(url[url.index(after: comma)...]))
            else { self.fail("\(out): \(error.map { "\($0)" } ?? "no picture")") }
            do { try (kind == "opaque" ? self.opaque(png, out) : png).write(to: URL(fileURLWithPath: out)) } catch { self.fail("\(out): \(error)") }
            self.next()
        }
    }
}

let app = NSApplication.shared
app.setActivationPolicy(.prohibited)
let renderer = Renderer()
renderer.start()
app.run()
