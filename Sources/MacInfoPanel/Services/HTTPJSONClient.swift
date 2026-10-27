import Foundation

private final class RedirectBlocker: NSObject, URLSessionTaskDelegate, @unchecked Sendable {
    func urlSession(
        _ session: URLSession,
        task: URLSessionTask,
        willPerformHTTPRedirection response: HTTPURLResponse,
        newRequest request: URLRequest,
        completionHandler: @escaping (URLRequest?) -> Void
    ) {
        completionHandler(nil)
    }
}

struct ResolvedField: Identifiable, Sendable {
    let id: String
    let label: String
    let text: String
    let numericValue: Double?
    let style: PanelConfiguration.HTTPJSONSource.Field.Style
    let positive: Bool?
}

struct HTTPJSONSnapshot: Sendable {
    let sourceID: String
    let title: String
    let symbol: String
    let fields: [ResolvedField]
    let receivedAt: Date
}

struct HTTPJSONClient: Sendable {
    enum ClientError: LocalizedError {
        case invalidURL(String)
        case badStatus(Int)
        case responseTooLarge
        case invalidJSON

        var errorDescription: String? {
            switch self {
            case .invalidURL(let reason): return reason
            case .badStatus(let status): return "HTTP \(status)"
            case .responseTooLarge: return "响应超过 1 MiB 限制"
            case .invalidJSON: return "响应不是有效 JSON"
            }
        }
    }

    let source: PanelConfiguration.HTTPJSONSource

    func fetch() async throws -> HTTPJSONSnapshot {
        try Self.validateURL(source.url)
        let configuration = URLSessionConfiguration.ephemeral
        configuration.timeoutIntervalForRequest = 8
        configuration.timeoutIntervalForResource = 12
        configuration.httpCookieStorage = nil
        configuration.urlCache = nil
        configuration.requestCachePolicy = .reloadIgnoringLocalCacheData
        configuration.httpAdditionalHeaders = ["Accept": "application/json"]
        let session = URLSession(configuration: configuration, delegate: RedirectBlocker(), delegateQueue: nil)
        defer { session.finishTasksAndInvalidate() }

        var request = URLRequest(url: source.url)
        request.httpMethod = "GET"
        let (data, response) = try await session.data(for: request)
        guard let http = response as? HTTPURLResponse else { throw ClientError.invalidJSON }
        guard (200..<300).contains(http.statusCode) else { throw ClientError.badStatus(http.statusCode) }
        guard data.count <= 1_048_576 else { throw ClientError.responseTooLarge }
        guard let json = try? JSONSerialization.jsonObject(with: data, options: [.fragmentsAllowed]) else {
            throw ClientError.invalidJSON
        }

        let fields = source.fields.map { field in
            resolve(field: field, in: json)
        }
        return HTTPJSONSnapshot(
            sourceID: source.id,
            title: source.title,
            symbol: source.symbol,
            fields: fields,
            receivedAt: Date()
        )
    }

    static func validateURL(_ url: URL) throws {
        guard url.user == nil, url.password == nil else {
            throw ClientError.invalidURL("URL 不能包含用户名或密码")
        }
        guard let scheme = url.scheme?.lowercased(), let host = url.host?.lowercased() else {
            throw ClientError.invalidURL("数据源 URL 无效")
        }
        if scheme == "https" { return }
        if scheme == "http", ["127.0.0.1", "localhost", "::1"].contains(host) { return }
        throw ClientError.invalidURL("只允许 HTTPS，明文 HTTP 仅限本机回环地址")
    }

    private func resolve(field: PanelConfiguration.HTTPJSONSource.Field, in json: Any) -> ResolvedField {
        let raw = value(at: field.pointer, in: json)
        let converted = format(raw, field: field)
        return ResolvedField(
            id: field.id,
            label: field.label,
            text: converted.text,
            numericValue: converted.number,
            style: field.style,
            positive: converted.positive
        )
    }

    private func value(at pointer: String, in root: Any) -> Any? {
        if pointer.isEmpty { return root }
        let tokens = pointer.dropFirst().split(separator: "/", omittingEmptySubsequences: false).map {
            $0.replacingOccurrences(of: "~1", with: "/").replacingOccurrences(of: "~0", with: "~")
        }
        return tokens.reduce(Optional(root)) { current, token in
            guard let current else { return nil }
            if let object = current as? [String: Any] { return object[token] }
            if let array = current as? [Any], let index = Int(token), array.indices.contains(index) { return array[index] }
            return nil
        }
    }

    private func format(
        _ raw: Any?,
        field: PanelConfiguration.HTTPJSONSource.Field
    ) -> (text: String, number: Double?, positive: Bool?) {
        switch field.type {
        case .boolean:
            let value = boolean(raw)
            return (
                value.map { $0 ? (field.trueText ?? "正常") : (field.falseText ?? "异常") } ?? "—",
                value.map { $0 ? 1 : 0 },
                value
            )
        case .percent:
            let value = number(raw).map { min(100, max(0, $0)) }
            return (value.map { String(format: "%.0f%%", $0) } ?? "—", value, nil)
        case .bytes:
            let value = number(raw)
            let text = value.map { ByteCountFormatter.string(fromByteCount: Int64(max(0, $0)), countStyle: .file) } ?? "—"
            return (text, value, nil)
        case .duration:
            let value = number(raw)
            return (value.map(duration) ?? "—", value, nil)
        case .date:
            guard let text = string(raw), text.count <= 200 else { return ("—", nil, nil) }
            if let date = ISO8601DateFormatter().date(from: text) {
                return (date.formatted(date: .abbreviated, time: .shortened), date.timeIntervalSince1970, nil)
            }
            return (text, nil, nil)
        case .number:
            let value = number(raw)
            let suffix = field.unit.map { " \($0)" } ?? ""
            return (value.map { String(format: $0.rounded() == $0 ? "%.0f%@" : "%.2f%@", $0, suffix) } ?? "—", value, nil)
        case .string:
            let value = string(raw).map { String($0.prefix(200)) }
            return ((value ?? "—") + (value != nil ? (field.unit.map { " \($0)" } ?? "") : ""), nil, nil)
        }
    }

    private func string(_ value: Any?) -> String? {
        if let value = value as? String { return value }
        if let value = value as? NSNumber { return value.stringValue }
        return nil
    }

    private func number(_ value: Any?) -> Double? {
        let result: Double?
        if let value = value as? NSNumber { result = value.doubleValue }
        else if let value = value as? String { result = Double(value) }
        else { result = nil }
        guard let result, result.isFinite else { return nil }
        return result
    }

    private func boolean(_ value: Any?) -> Bool? {
        if let value = value as? Bool { return value }
        if let value = value as? NSNumber { return value.boolValue }
        if let value = value as? String {
            switch value.lowercased() {
            case "true", "yes", "online", "up", "ok", "healthy", "running", "1": return true
            case "false", "no", "offline", "down", "error", "unhealthy", "stopped", "0": return false
            default: return nil
            }
        }
        return nil
    }

    private func duration(_ seconds: Double) -> String {
        let totalMinutes = max(0, Int(seconds)) / 60
        let days = totalMinutes / 1_440
        let hours = (totalMinutes % 1_440) / 60
        let minutes = totalMinutes % 60
        if days > 0 { return "\(days)天 \(hours)小时" }
        if hours > 0 { return "\(hours)小时 \(minutes)分" }
        return "\(minutes)分钟"
    }
}
