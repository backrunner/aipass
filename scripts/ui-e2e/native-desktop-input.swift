// Send events only to the verified disposable app, even when another window has focus.
import ApplicationServices
import Foundation

func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data((message + "\n").utf8))
    exit(1)
}

let args = CommandLine.arguments
guard args.count >= 4, let pid = Int32(args[2]),
      args[1].hasPrefix("/private/tmp/aipass-runtime-") else { fail("Missing fixture PID or path") }
// Launch Services can resolve a duplicate bundle identifier to the installed
// application. Always verify the actual executable in the kernel process table.
var pathBuffer = [CChar](repeating: 0, count: 4096)
guard proc_pidpath(pid, &pathBuffer, UInt32(pathBuffer.count)) > 0 else { fail("Cannot resolve fixture PID") }
let executable = URL(fileURLWithPath: String(cString: pathBuffer)).resolvingSymlinksInPath().path
let expected = URL(fileURLWithPath: args[1] + "/AIPass.app/Contents/MacOS/aipass-desktop").resolvingSymlinksInPath().path
guard executable == expected else { fail("Unexpected fixture executable " + executable) }
guard AXIsProcessTrusted() else { fail("Native UI input requires Accessibility permission for the invoking terminal") }
let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
guard let window = windows.first(where: {
    ($0[kCGWindowOwnerPID as String] as? Int32) == pid &&
    ($0[kCGWindowLayer as String] as? Int) == 0 &&
    (($0[kCGWindowBounds as String] as? [String: Any])?["Width"] as? Double ?? 0) >= 950
}), let number = window[kCGWindowNumber as String] as? UInt32 else { fail("No visible fixture window") }
func key(_ code: UInt16, digit: Character? = nil) {
    for down in [true, false] {
        guard let event = CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: down)
            else { fail("Cannot create fixture key event") }
        event.flags = []
        if down, let digit {
            let units = Array(String(digit).utf16)
            event.keyboardSetUnicodeString(stringLength: units.count, unicodeString: units)
        }
        event.postToPid(pid)
        Thread.sleep(forTimeInterval: 0.04)
    }
}
switch args[3] {
case "digits":
    let digits = String(data: FileHandle.standardInput.readDataToEndOfFile(), encoding: .utf8) ?? ""
    let codes: [UInt16] = [29, 18, 19, 20, 21, 23, 22, 26, 28, 25]
    guard !digits.isEmpty, digits.count <= 40 else { fail("Expected fixture digits") }
    for digit in digits {
        guard let index = digit.wholeNumberValue, index < codes.count else { fail("Expected a digit") }
        key(codes[index], digit: digit)
    }
case "enter": key(36)
case "tab": key(48)
case "delete":
    guard args.count == 5, let count = Int(args[4]), (1...40).contains(count) else { fail("Invalid deletion count") }
    for _ in 0..<count { key(51) }
case "screen-locked":
    let session = CGSessionCopyCurrentDictionary() as? [String: Any]
    print(session?["CGSSessionScreenIsLocked"] as? Bool == true ? "true" : "false")
case "screen-capture-access": print(CGPreflightScreenCaptureAccess() ? "true" : "false")
case "window": print(number)
default: fail("Unknown input operation")
}
