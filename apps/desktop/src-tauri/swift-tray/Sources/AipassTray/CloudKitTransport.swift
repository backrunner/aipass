import AppKit
import CloudKit
import CryptoKit
import Security
import ObjectiveC.runtime

// This native boundary only transports ciphertext. It never receives vault keys.
private let cloudContainer = Bundle.main.object(forInfoDictionaryKey: "AIPassCloudKitContainer") as? String ?? "iCloud.com.alkinum.aipass"
private let cloudZone = CKRecordZone.ID(zoneName: "AIPassVault", ownerName: CKCurrentUserDefaultName)
private let cloudSubscription = "aipass-vault-changes-v1"
private let maxSnapshotBytes = 11 * 1024 * 1024
private func digest(_ data: Data) -> String { SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined() }
private func validID(_ id: String) -> Bool { id.count == 64 && id.allSatisfy { "0123456789abcdef".contains($0) } }

private struct TransportError: Error { let message: String; var kind: String = "invalid_data" }
private struct TransportTask: Decodable {
    struct Command: Decodable { let kind: String; let id: String?; let bytes_b64: String? }
    let command: Command
    let account: String?
}
private struct TransportReply: Encodable {
    var ids: [String] = []
    var account: String?
    var bytes_b64: String?
    var error: String?
    var error_kind: String?
}

private func hasCloudEntitlement() -> Bool {
    guard let task = SecTaskCreateFromSelf(nil),
          let containers = SecTaskCopyValueForEntitlement(task, "com.apple.developer.icloud-container-identifiers" as CFString, nil) as? [String] else { return false }
    return containers.contains(cloudContainer)
}

// The notification invalidates in-flight work synchronously, including while
// the actor is suspended in a CloudKit await.
private final class AccountGeneration: @unchecked Sendable {
    static let shared = AccountGeneration()
    private let lock = NSLock()
    private var generation: UInt64 = 0
    func current() -> UInt64 { lock.lock(); defer { lock.unlock() }; return generation }
    func invalidate() { lock.lock(); defer { lock.unlock() }; generation &+= 1 }
}

private actor CloudTransport {
    static let shared = CloudTransport()
    private var account: String?
    private var token: CKServerChangeToken?
    private var ids: Set<String> = []
    private var subscribed = false
    private var retryUntil = Date.distantPast
    private var subscriptionRetryUntil = Date.distantPast
    private var accountGeneration: UInt64?

    private func checkActive(_ generation: UInt64) throws {
        try Task.checkCancellation()
        guard AccountGeneration.shared.current() == generation else {
            throw TransportError(message: "iCloud account changed during synchronization; retry", kind: "account_changed")
        }
    }

    private func verifyAccount(_ container: CKContainer, _ expected: String, _ generation: UInt64) async throws {
        try checkActive(generation)
        let identity = try await container.userRecordID()
        try checkActive(generation)
        guard digest(Data(identity.recordName.utf8)) == expected else {
            throw TransportError(message: "iCloud account changed during synchronization; retry", kind: "account_changed")
        }
    }

    func deferRetry(_ seconds: Double) { retryUntil = max(retryUntil, Date().addingTimeInterval(max(0, seconds))) }

    func execute(_ task: TransportTask) async throws -> TransportReply {
        let generation = AccountGeneration.shared.current()
        try checkActive(generation)
        // CKContainer raises an Objective-C exception without an entitlement.
        // A development/unsigned build must report unavailable before calling it.
        guard hasCloudEntitlement() else {
            throw TransportError(message: "CloudKit requires a signed AIPass app with its iCloud provisioning profile", kind: "unavailable")
        }
        guard Date() >= retryUntil else { throw TransportError(message: "CloudKit requested a retry delay", kind: "unavailable") }
        let container = CKContainer(identifier: cloudContainer)
        guard try await container.accountStatus() == .available else {
            throw TransportError(message: "Sign in to iCloud in macOS System Settings", kind: "authentication")
        }
        try checkActive(generation)
        let identity = try await container.userRecordID()
        try checkActive(generation)
        let nextAccount = digest(Data(identity.recordName.utf8))
        if account != nextAccount || accountGeneration != generation {
            token = nil; ids = []; subscribed = false; account = nextAccount
            accountGeneration = generation; subscriptionRetryUntil = .distantPast
        }
        if let expected = task.account, expected != nextAccount {
            throw TransportError(message: "iCloud account changed during synchronization; retry", kind: "account_changed")
        }
        try checkActive(generation)
        let database = container.privateCloudDatabase
        if !subscribed && Date() >= subscriptionRetryUntil {
            let subscription = CKDatabaseSubscription(subscriptionID: cloudSubscription)
            let info = CKSubscription.NotificationInfo()
            info.shouldSendContentAvailable = true
            subscription.notificationInfo = info
            // Polling continues if silent-notification registration is unavailable.
            do {
                _ = try await database.save(subscription)
                try checkActive(generation)
                subscribed = true
            } catch {
                try checkActive(generation)
                subscriptionRetryUntil = Date().addingTimeInterval(max(60, (error as? CKError)?.retryAfterSeconds ?? 0))
            }
        }
        switch task.command.kind {
        case "list":
            var reset = false
            var nextToken = token
            var nextIDs = ids
            while true {
                try checkActive(generation)
                do {
                    let changes = try await database.recordZoneChanges(inZoneWith: cloudZone, since: nextToken, desiredKeys: [], resultsLimit: 200)
                    try checkActive(generation)
                    for (id, result) in changes.modificationResultsByID {
                        _ = try result.get()
                        if validID(id.recordName) { nextIDs.insert(id.recordName) }
                    }
                    for deletion in changes.deletions { nextIDs.remove(deletion.recordID.recordName) }
                    nextToken = changes.changeToken
                    if !changes.moreComing { break }
                } catch let error as CKError where error.code == .changeTokenExpired && !reset {
                    reset = true; nextToken = nil; nextIDs = []
                } catch let error as CKError where error.code == .zoneNotFound || error.code == .userDeletedZone {
                    nextToken = nil; nextIDs = []; break
                }
            }
            // Commit cursor only after every page and per-record result succeeds.
            try await verifyAccount(container, nextAccount, generation)
            token = nextToken; ids = nextIDs
            return TransportReply(ids: ids.sorted(), account: nextAccount)
        case "get":
            guard let id = task.command.id, validID(id) else { throw TransportError(message: "Invalid snapshot identifier") }
            let record = try await database.record(for: CKRecord.ID(recordName: id, zoneID: cloudZone))
            try checkActive(generation)
            let data = try readAsset(record, id: id)
            try await verifyAccount(container, nextAccount, generation)
            return TransportReply(account: nextAccount, bytes_b64: data.base64EncodedString())
        case "put":
            guard let id = task.command.id, validID(id), let text = task.command.bytes_b64,
                  let data = Data(base64Encoded: text), data.count <= maxSnapshotBytes, digest(data) == id else {
                throw TransportError(message: "Invalid encrypted snapshot")
            }
            _ = try await database.save(CKRecordZone(zoneID: cloudZone))
            try checkActive(generation)
            let file = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
            try data.write(to: file, options: .atomic)
            defer { try? FileManager.default.removeItem(at: file) }
            let record = CKRecord(recordType: "VaultSnapshot", recordID: CKRecord.ID(recordName: id, zoneID: cloudZone))
            record["ciphertext"] = CKAsset(fileURL: file)
            do {
                let result = try await database.modifyRecords(saving: [record], deleting: [], savePolicy: .ifServerRecordUnchanged, atomically: true)
                guard let saved = result.saveResults[record.recordID] else { throw TransportError(message: "CloudKit did not acknowledge the snapshot") }
                _ = try saved.get()
            } catch {
                // Retry after a lost response is idempotent. Never overwrite an
                // existing record with different bytes, even if its name matches.
                let original = error
                try checkActive(generation)
                do {
                    let existing = try await database.record(for: record.recordID)
                    guard try readAsset(existing, id: id) == data else { throw original }
                } catch { throw original }
            }
            try await verifyAccount(container, nextAccount, generation)
            ids.insert(id)
            return TransportReply(account: nextAccount)
        default: throw TransportError(message: "Unknown CloudKit transport operation")
        }
    }

    private func readAsset(_ record: CKRecord, id: String) throws -> Data {
        guard record.recordType == "VaultSnapshot", let asset = record["ciphertext"] as? CKAsset, let url = asset.fileURL else { throw TransportError(message: "CloudKit ciphertext asset missing") }
        let size = try url.resourceValues(forKeys: [.fileSizeKey]).fileSize ?? 0
        guard size <= maxSnapshotBytes else { throw TransportError(message: "CloudKit snapshot exceeds the transport size limit") }
        let data = try Data(contentsOf: url)
        guard digest(data) == id else { throw TransportError(message: "CloudKit snapshot integrity check failed") }
        return data
    }
}

// A timed-out CloudKit operation may finish later. Its result owns this box,
// never a stack pointer, and cancellation is checked before committing cursors.
private final class ReplyBox: @unchecked Sendable {
    private let lock = NSLock()
    private var value = TransportReply(error: "CloudKit operation timed out", error_kind: "unavailable")
    func set(_ reply: TransportReply) { lock.lock(); defer { lock.unlock() }; value = reply }
    func get() -> TransportReply { lock.lock(); defer { lock.unlock() }; return value }
}

@_cdecl("aipass_cloudkit_execute")
public func aipassCloudKitExecute(_ input: UnsafePointer<CChar>) -> UnsafeMutablePointer<CChar>? {
    let bytes = Data(String(cString: input).utf8)
    let semaphore = DispatchSemaphore(value: 0)
    let output = ReplyBox()
    let operation = Task {
        do { output.set(try await CloudTransport.shared.execute(JSONDecoder().decode(TransportTask.self, from: bytes))) }
        catch let error as TransportError { output.set(TransportReply(error: error.message, error_kind: error.kind)) }
        catch let error as CKError {
            if let retry = error.retryAfterSeconds { await CloudTransport.shared.deferRetry(retry) }
            output.set(TransportReply(error: "CloudKit error \(error.code.rawValue); retry after \(error.retryAfterSeconds ?? 0) seconds", error_kind: error.code == .notAuthenticated || error.code == .permissionFailure ? "authentication" : "unavailable"))
        }
        catch { output.set(TransportReply(error: "CloudKit transport failed", error_kind: "unavailable")) }
        semaphore.signal()
    }
    let reply: TransportReply
    if semaphore.wait(timeout: .now() + 30) == .timedOut {
        operation.cancel()
        reply = TransportReply(error: "CloudKit operation timed out", error_kind: "unavailable")
    } else { reply = output.get() }
    let encoded = (try? JSONEncoder().encode(reply)) ?? Data("{\"error\":\"CloudKit encoding failed\"}".utf8)
    return strdup(String(decoding: encoded, as: UTF8.self))
}

@_cdecl("aipass_cloudkit_free")
public func aipassCloudKitFree(_ pointer: UnsafeMutablePointer<CChar>?) { free(pointer) }

private var cloudWake: (@convention(c) () -> Void)?
private var accountObserver: NSObjectProtocol?

@_cdecl("aipass_cloudkit_observe")
public func aipassCloudKitObserve(_ callback: @escaping @convention(c) () -> Void) {
    guard cloudWake == nil else { return }
    cloudWake = callback
    accountObserver = NotificationCenter.default.addObserver(forName: .CKAccountChanged, object: nil, queue: nil) { _ in
        AccountGeneration.shared.invalidate()
        cloudWake?()
    }
    guard hasCloudEntitlement(), let delegate = NSApplication.shared.delegate, let cls = object_getClass(delegate) else { return }
    let selector = NSSelectorFromString("application:didReceiveRemoteNotification:")
    let prior = class_getInstanceMethod(cls, selector).map(method_getImplementation)
    let block: @convention(block) (AnyObject, NSApplication, NSDictionary) -> Void = { object, app, info in
        if let dictionary = info as? [String: NSObject], let notification = CKNotification(fromRemoteNotificationDictionary: dictionary), notification.containerIdentifier == cloudContainer { cloudWake?() }
        if let prior {
            typealias Handler = @convention(c) (AnyObject, Selector, NSApplication, NSDictionary) -> Void
            unsafeBitCast(prior, to: Handler.self)(object, selector, app, info)
        }
    }
    let implementation = imp_implementationWithBlock(block)
    class_replaceMethod(cls, selector, implementation, "v@:@@")
    NSApplication.shared.registerForRemoteNotifications()
}
