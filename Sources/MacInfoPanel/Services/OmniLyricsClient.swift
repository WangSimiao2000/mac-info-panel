import Foundation

struct OmniLyricsClient: Sendable {
    let url: URL

    func fetch() async -> LyricSnapshot {
        do {
            var request = URLRequest(url: url)
            request.cachePolicy = .reloadIgnoringLocalCacheData
            request.timeoutInterval = 2
            let (data, response) = try await URLSession.shared.data(for: request)
            guard let http = response as? HTTPURLResponse, http.statusCode == 200 else {
                return failure("OmniLyrics 接口不可用")
            }
            guard let root = try JSONSerialization.jsonObject(with: data) as? [String: Any] else {
                return failure("OmniLyrics 返回格式无效")
            }
            return parse(root)
        } catch {
            return failure("等待 OmniLyrics")
        }
    }

    private func parse(_ root: [String: Any]) -> LyricSnapshot {
        guard let state = root["state"] as? [String: Any] else {
            return LyricSnapshot()
        }

        let title = string(state["title"]) ?? "未知歌曲"
        let artists: String = {
            if let values = state["artists"] as? [String] { return values.joined(separator: " · ") }
            return string(state["artist"]) ?? "未知歌手"
        }()
        let position = seconds(state["position"]) ?? seconds(state["positionSeconds"]) ?? 0
        let duration = seconds(state["duration"]) ?? seconds(state["durationSeconds"]) ?? 0
        let playing = (state["playing"] as? Bool) ?? false

        let rawLyrics: [Any] = {
            if let lines = root["lyrics"] as? [Any] { return lines }
            if let lyrics = root["lyrics"] as? [String: Any], let lines = lyrics["lines"] as? [Any] { return lines }
            return []
        }()

        let lines = rawLyrics.compactMap { value -> (time: Double, text: String, translation: String)? in
            guard let item = value as? [String: Any] else { return nil }
            guard let time = seconds(item["timestamp"]) ?? seconds(item["time"]) ?? seconds(item["startTime"]) else { return nil }
            guard let text = string(item["text"]), !text.isEmpty else { return nil }
            let translation = string(item["translation"]) ?? string(item["translatedText"]) ?? ""
            return (time, text, translation)
        }.sorted { $0.time < $1.time }

        let index = lines.lastIndex { $0.time <= position }
        let current = index.map { lines[$0] }
        let next = index.flatMap { $0 + 1 < lines.count ? lines[$0 + 1] : nil } ?? (index == nil ? lines.first : nil)

        return LyricSnapshot(
            available: true,
            playing: playing,
            title: title,
            artists: artists,
            currentLine: current?.text ?? (lines.isEmpty ? "暂未匹配到歌词" : "♪"),
            currentTranslation: current?.translation ?? "",
            nextLine: next?.text ?? "",
            progress: duration > 0 ? min(1, max(0, position / duration)) : 0,
            error: nil
        )
    }

    private func failure(_ message: String) -> LyricSnapshot {
        var snapshot = LyricSnapshot()
        snapshot.error = message
        return snapshot
    }

    private func string(_ value: Any?) -> String? {
        if let value = value as? String { return value }
        if let value = value as? NSNumber { return value.stringValue }
        return nil
    }

    private func seconds(_ value: Any?) -> Double? {
        if let value = value as? NSNumber { return value.doubleValue }
        guard let text = value as? String else { return nil }
        if let number = Double(text) { return number }
        let parts = text.split(separator: ":").compactMap { Double($0) }
        guard parts.count == 2 || parts.count == 3 else { return nil }
        return parts.reduce(0) { $0 * 60 + $1 }
    }
}
