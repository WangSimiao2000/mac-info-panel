import Foundation

struct MCPanelService: Sendable {
    enum ServiceError: LocalizedError {
        case keyMissing
        case timeout
        case sshFailure(String)
        case invalidResponse

        var errorDescription: String? {
            switch self {
            case .keyMissing: return "等待配置 mc-ops 专用 SSH Key"
            case .timeout: return "mc-ops SSH 请求超时"
            case .sshFailure(let detail): return detail.isEmpty ? "mc-ops SSH 连接失败" : detail
            case .invalidResponse: return "mc-ops 状态格式无效"
            }
        }
    }

    let configuration: PanelConfiguration

    func fetch() async throws -> MCPanelSnapshot {
        try await Task.detached(priority: .utility) {
            guard FileManager.default.fileExists(atPath: configuration.sshKeyPath) else {
                throw ServiceError.keyMissing
            }

            let process = Process()
            process.executableURL = URL(fileURLWithPath: "/usr/bin/ssh")
            process.arguments = [
                "-T",
                "-o", "BatchMode=yes",
                "-o", "ConnectTimeout=5",
                "-o", "ConnectionAttempts=1",
                "-o", "IdentitiesOnly=yes",
                "-o", "ClearAllForwardings=yes",
                "-o", "StrictHostKeyChecking=yes",
                "-i", configuration.sshKeyPath,
                "\(configuration.sshUser)@\(configuration.sshHost)",
                configuration.sshCommand
            ]

            let stdout = Pipe()
            let stderr = Pipe()
            process.standardOutput = stdout
            process.standardError = stderr
            try process.run()

            let deadline = Date().addingTimeInterval(8)
            while process.isRunning && Date() < deadline {
                try await Task.sleep(nanoseconds: 50_000_000)
            }
            if process.isRunning {
                process.terminate()
                throw ServiceError.timeout
            }

            let output = stdout.fileHandleForReading.readDataToEndOfFile()
            let errorData = stderr.fileHandleForReading.readDataToEndOfFile()
            guard process.terminationStatus == 0 else {
                let detail = String(data: errorData, encoding: .utf8)?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
                throw ServiceError.sshFailure(detail)
            }
            guard let snapshot = try? JSONDecoder().decode(MCPanelSnapshot.self, from: output), snapshot.schema == 1 else {
                throw ServiceError.invalidResponse
            }
            return snapshot
        }.value
    }
}
