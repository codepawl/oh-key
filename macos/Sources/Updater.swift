import Cocoa

/// Looks for a newer release and tells the user through the input menu. An input method cannot
/// replace itself while the system has it loaded, so the menu item opens the release page.
/// The newest tag is where https://github.com/codepawl/oh-key-releases/releases/latest redirects.
enum Updater {
    private static let firstCheckDelay: TimeInterval = 10
    private static let checkInterval: TimeInterval = 6 * 3600

    private static var timer: Timer?
    private static var availableTag: String?
    private static var lastCheckFailed = false
    private static var lastCheckFoundNothing = false
    private static var checking = false

    static var menuTitle: String {
        if let tag = availableTag { return "Có bản mới \(tag): mở trang tải" }
        if checking { return "Đang kiểm tra cập nhật…" }
        if lastCheckFailed { return "Không kiểm tra được cập nhật (thử lại)" }
        if lastCheckFoundNothing { return "Đã là bản mới nhất (kiểm tra lại)" }
        return "Kiểm tra cập nhật"
    }

    /// Checks 10 seconds after start, then every 6 hours, unless the config says updates = off.
    static func start() {
        DispatchQueue.main.asyncAfter(deadline: .now() + firstCheckDelay) {
            checkIfAllowed()
            timer = Timer.scheduledTimer(withTimeInterval: checkInterval, repeats: true) { _ in checkIfAllowed() }
        }
    }

    /// The menu item: opens the release page when a newer version is known, else checks now.
    static func menuItemChosen() {
        if let tag = availableTag, let url = URL(string: UpdateCheck.releasePageBase + tag) {
            NSWorkspace.shared.open(url)
            return
        }
        check()
    }

    private static func checkIfAllowed() {
        let mode = ReportingConfig.updateMode(in: Preferences.readText(at: Preferences.configURL))
        if mode == .off { return }
        check()
    }

    private static func check() {
        if checking { return }
        checking = true
        fetchLatestTag { result in
            DispatchQueue.main.async {
                checking = false
                apply(result)
            }
        }
    }

    private static func apply(_ result: Result<String, Error>) {
        lastCheckFailed = false
        lastCheckFoundNothing = false
        switch result {
        case .success(let tag):
            if UpdateCheck.isNewer(tag, than: Reporter.appVersion) {
                availableTag = tag
                AppLog.write("update available: \(tag)")
            } else {
                lastCheckFoundNothing = true
            }
        case .failure(let error):
            lastCheckFailed = true
            AppLog.write("update check did not finish: \(error.localizedDescription)")
        }
    }

    private static func fetchLatestTag(completion: @escaping (Result<String, Error>) -> Void) {
        guard let url = URL(string: UpdateCheck.releasesURL) else { return }
        var request = URLRequest(url: url, timeoutInterval: 30)
        request.httpMethod = "HEAD"
        let configuration = URLSessionConfiguration.ephemeral
        let session = URLSession(configuration: configuration, delegate: RedirectCapture(completion), delegateQueue: nil)
        session.dataTask(with: request).resume()
    }
}

/// Stops at the first redirect and reads where it points, instead of following it.
private final class RedirectCapture: NSObject, URLSessionTaskDelegate {
    private let completion: (Result<String, Error>) -> Void
    private var finished = false

    init(_ completion: @escaping (Result<String, Error>) -> Void) {
        self.completion = completion
    }

    func urlSession(
        _ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse,
        newRequest request: URLRequest, completionHandler: @escaping (URLRequest?) -> Void
    ) {
        completionHandler(nil)
        let location = request.url?.absoluteString ?? ""
        if let tag = UpdateCheck.tag(fromReleaseURL: location) {
            finish(.success(tag))
        } else {
            finish(.failure(UpdateError.noTag))
        }
    }

    func urlSession(_ session: URLSession, task: URLSessionTask, didCompleteWithError error: Error?) {
        session.finishTasksAndInvalidate()
        finish(.failure(error ?? UpdateError.noTag))
    }

    private func finish(_ result: Result<String, Error>) {
        if finished { return }
        finished = true
        completion(result)
    }
}

private enum UpdateError: Error, LocalizedError {
    case noTag

    var errorDescription: String? { "the release page gave no tag" }
}
