import Foundation

struct MCPanelService: Sendable {
    enum ServiceError: LocalizedError {
        case timeout
        case sshFailure(String)
        case invalidResponse
        case allSourcesUnavailable

        var errorDescription: String? {
            switch self {
            case .timeout: return "mc-ops SSH 请求超时"
            case .sshFailure(let detail): return detail.isEmpty ? "mc-ops SSH 连接失败" : detail
            case .invalidResponse: return "mc-ops 状态格式无效"
            case .allSourcesUnavailable: return "mc-ops SSH 与公开状态均不可用"
            }
        }
    }

    private struct PublicStatus: Decodable {
        let online: Bool
        let state: String
        let players_online: Int
        let players_max: Int
        let version: String?
        let uptime_s: Double?
        let updated_at: String?
    }

    let configuration: PanelConfiguration

    func fetch() async throws -> MCPanelSnapshot {
        if FileManager.default.fileExists(atPath: configuration.sshKeyPath),
           let snapshot = try? await fetchSSH() {
            return snapshot
        }
        if let snapshot = try? await fetchPublicStatus() {
            return snapshot
        }
        throw ServiceError.allSourcesUnavailable
    }

    private func fetchSSH() async throws -> MCPanelSnapshot {
        try await Task.detached(priority: .utility) {
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

    private func fetchPublicStatus() async throws -> MCPanelSnapshot {
        var request = URLRequest(url: configuration.publicStatusURL)
        request.cachePolicy = .reloadRevalidatingCacheData
        request.timeoutInterval = 5
        let (data, response) = try await URLSession.shared.data(for: request)
        guard let http = response as? HTTPURLResponse, http.statusCode == 200 else {
            throw ServiceError.invalidResponse
        }
        let status = try JSONDecoder().decode(PublicStatus.self, from: data)
        let stale = status.updated_at.flatMap(ISO8601DateFormatter().date).map { Date().timeIntervalSince($0) > 1_200 } ?? true
        return MCPanelSnapshot(
            schema: 1,
            generatedAt: status.updated_at,
            agent: .init(health: "公开状态", version: nil, serviceActive: nil),
            minecraft: .init(
                state: status.state,
                serviceActive: status.online,
                online: status.online,
                playersOnline: status.players_online,
                playersMax: status.players_max,
                version: status.version,
                uptimeSeconds: status.uptime_s
            ),
            host: nil,
            warning: stale ? "公开状态已超过 20 分钟未更新" : "公开状态 · 最多约 10 分钟延迟"
        )
    }
}
