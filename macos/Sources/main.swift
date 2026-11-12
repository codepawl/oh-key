import Cocoa
import InputMethodKit

AppLog.write("started oh-key \(Reporter.appVersion)")
CrashCapture.reportPending()
CrashCapture.install()
Updater.start()

// The server lives as long as the process; the system connects to it by name.
let connectionName = Bundle.main.infoDictionary?["InputMethodConnectionName"] as? String
let inputMethodServer = IMKServer(name: connectionName, bundleIdentifier: Bundle.main.bundleIdentifier)

withExtendedLifetime(inputMethodServer) {
    NSApplication.shared.run()
}
