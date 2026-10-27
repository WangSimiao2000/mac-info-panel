import Foundation

struct PlaybackState: Sendable, Equatable {
    let title: String
    let artist: String
    let album: String
    let bundleIdentifier: String
    let contentIdentifier: String
    let duration: Double
    let position: Double
    let playing: Bool
    let sampledAt: Date

    var trackKey: String {
        [title, artist, album, String(Int(duration.rounded()))]
            .map { $0.folding(options: [.caseInsensitive, .diacriticInsensitive], locale: .current) }
            .joined(separator: "|")
    }
}

struct MediaControlClient: Sendable {
    enum ClientError: LocalizedError {
        case executableMissing
        case launchFailed(String)

        var errorDescription: String? {
            switch self {
            case .executableMissing: return "未找到 media-control，请先运行 brew install media-control"
            case .launchFailed(let detail): return detail
            }
        }
    }

    let configuredPath: String?

    func updates() -> AsyncStream<Result<PlaybackState?, Error>> {
        AsyncStream { continuation in
            do {
                let executable = try executableURL()
                let monitor = MediaControlMonitor(executable: executable, continuation: continuation)
                continuation.onTermination = { @Sendable _ in monitor.stop() }
                monitor.start()
            } catch {
                continuation.yield(.failure(error))
                continuation.finish()
            }
        }
    }

    private func executableURL() throws -> URL {
        let candidates = [configuredPath, "/opt/homebrew/bin/media-control", "/usr/local/bin/media-control"]
            .compactMap { $0 }
            .map { NSString(string: $0).expandingTildeInPath }
        for path in candidates where FileManager.default.isExecutableFile(atPath: path) {
            return URL(fileURLWithPath: path)
        }
        throw ClientError.executableMissing
    }
}

private final class MediaControlMonitor: @unchecked Sendable {
    private struct Envelope: Decodable {
        let type: String?
        let payload: Payload?
    }

    private struct Payload: Decodable {
        let title: String?
        let artist: String?
        let album: String?
        let bundleIdentifier: String?
        let contentItemIdentifier: String?
        let durationMicros: Int64?
        let elapsedTimeMicros: Int64?
        let timestampEpochMicros: Int64?
        let playing: Bool?
    }

    private let executable: URL
    private let continuation: AsyncStream<Result<PlaybackState?, Error>>.Continuation
    private let lock = NSLock()
    private var process: Process?
    private var stdout: Pipe?
    private var stderr: Pipe?
    private var buffer = Data()
    private var stopped = false

    init(executable: URL, continuation: AsyncStream<Result<PlaybackState?, Error>>.Continuation) {
        self.executable = executable
        self.continuation = continuation
    }

    func start() {
        let process = Process()
        let stdout = Pipe()
        let stderr = Pipe()
        process.executableURL = executable
        process.arguments = ["stream", "--micros", "--no-artwork", "--no-diff"]
        process.standardOutput = stdout
        process.standardError = stderr

        stdout.fileHandleForReading.readabilityHandler = { [weak self] handle in
            self?.consume(handle.availableData)
        }
        stderr.fileHandleForReading.readabilityHandler = { handle in
            _ = handle.availableData
        }
        process.terminationHandler = { [weak self] process in
            guard let self else { return }
            self.stopHandlers()
            if !self.isStopped {
                let detail = "media-control 已退出（code \(process.terminationStatus)），即将重试"
                self.continuation.yield(.failure(MediaControlClient.ClientError.launchFailed(detail)))
            }
            self.continuation.finish()
        }

        lock.lock()
        self.process = process
        self.stdout = stdout
        self.stderr = stderr
        lock.unlock()

        do {
            try process.run()
        } catch {
            stopHandlers()
            continuation.yield(.failure(error))
            continuation.finish()
        }
    }

    func stop() {
        lock.lock()
        stopped = true
        let process = self.process
        lock.unlock()
        stopHandlers()
        if process?.isRunning == true { process?.terminate() }
    }

    private var isStopped: Bool {
        lock.lock()
        defer { lock.unlock() }
        return stopped
    }

    private func stopHandlers() {
        lock.lock()
        let stdout = self.stdout
        let stderr = self.stderr
        lock.unlock()
        stdout?.fileHandleForReading.readabilityHandler = nil
        stderr?.fileHandleForReading.readabilityHandler = nil
    }

    private func consume(_ data: Data) {
        guard !data.isEmpty else { return }
        lock.lock()
        buffer.append(data)
        guard buffer.count <= 262_144 else {
            buffer.removeAll(keepingCapacity: true)
            lock.unlock()
            continuation.yield(.failure(MediaControlClient.ClientError.launchFailed("media-control 单行输出超过限制")))
            return
        }
        var lines: [Data] = []
        while let newline = buffer.firstIndex(of: 0x0A) {
            lines.append(buffer.prefix(upTo: newline))
            buffer.removeSubrange(...newline)
        }
        lock.unlock()

        for line in lines where !line.isEmpty {
            guard let envelope = try? JSONDecoder().decode(Envelope.self, from: line) else { continue }
            guard let payload = envelope.payload, let title = payload.title, !title.isEmpty else {
                continuation.yield(.success(nil))
                continue
            }
            let now = Date()
            let duration = Double(payload.durationMicros ?? 0) / 1_000_000
            let reported = Double(payload.elapsedTimeMicros ?? 0) / 1_000_000
            let timestamp = Double(payload.timestampEpochMicros ?? 0) / 1_000_000
            let lag = timestamp > 0 && payload.playing == true ? max(0, now.timeIntervalSince1970 - timestamp) : 0
            let position = min(duration > 0 ? duration : .greatestFiniteMagnitude, max(0, reported + lag))
            continuation.yield(.success(PlaybackState(
                title: title,
                artist: payload.artist ?? "",
                album: payload.album ?? "",
                bundleIdentifier: payload.bundleIdentifier ?? "",
                contentIdentifier: payload.contentItemIdentifier ?? "",
                duration: duration,
                position: position,
                playing: payload.playing ?? false,
                sampledAt: now
            )))
        }
    }
}
