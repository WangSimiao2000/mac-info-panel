import Foundation

struct OmniLyricsClient: Sendable {
    private struct Response: Decodable {
        let service: String
        let protocolVersion: Int
        let state: PlayerState?
        let lyrics: [Line]?
        let loading: Bool
        let lyricsVersion: String
    }

    private struct PlayerState: Decodable {
        let title: String
        let artists: [String]
        let position: String
        let duration: String
        let playing: Bool
    }

    private struct Line: Decodable {
        let timestamp: String
        let text: String
        let tokens: [Token]?
        let translations: [String: String]?
        let endTime: String?
    }

    private struct Token: Decodable {
        let startTime: String
        let duration: String
        let text: String
    }

    let url: URL

    func fetch() async -> LyricSnapshot {
        do {
            var request = URLRequest(url: url)
            request.cachePolicy = .reloadIgnoringLocalCacheData
            request.timeoutInterval = 2
            let (data, response) = try await URLSession.shared.data(for: request)
            guard data.count <= 1_048_576 else { return failure("OmniLyrics 响应超过 1 MiB") }
            guard let http = response as? HTTPURLResponse, http.statusCode == 200 else {
                return failure("OmniLyrics 接口不可用")
            }
            let payload = try JSONDecoder().decode(Response.self, from: data)
            guard payload.service == "OmniLyrics", payload.protocolVersion == 1 else {
                return failure("不兼容的 OmniLyrics 接口")
            }
            return makeSnapshot(payload)
        } catch {
            return failure("等待 OmniLyrics")
        }
    }

    private func makeSnapshot(_ response: Response) -> LyricSnapshot {
        guard let state = response.state else {
            return LyricSnapshot(
                apiAvailable: true,
                playerAvailable: false,
                loading: response.loading,
                sampledAt: Date(),
                lyricsVersion: response.lyricsVersion
            )
        }

        let lines = (response.lyrics ?? []).enumerated().compactMap { index, line -> LyricLine? in
            guard let timestamp = timeInterval(line.timestamp) else { return nil }
            let translation = line.translations?["zh"]
                ?? line.translations?["zh-CN"]
                ?? line.translations?["en"]
                ?? line.translations?.values.first
                ?? ""
            let tokens = (line.tokens ?? []).enumerated().compactMap { tokenIndex, token -> LyricToken? in
                guard let start = timeInterval(token.startTime), let duration = timeInterval(token.duration) else { return nil }
                return LyricToken(id: tokenIndex, startTime: start, duration: duration, text: token.text)
            }
            return LyricLine(
                id: index,
                startTime: timestamp,
                endTime: line.endTime.flatMap(timeInterval),
                text: line.text,
                translation: translation,
                tokens: tokens
            )
        }.sorted { $0.startTime < $1.startTime }

        return LyricSnapshot(
            apiAvailable: true,
            playerAvailable: true,
            loading: response.loading,
            playing: state.playing,
            title: state.title,
            artists: state.artists.joined(separator: " · "),
            positionSeconds: timeInterval(state.position) ?? 0,
            durationSeconds: timeInterval(state.duration) ?? 0,
            sampledAt: Date(),
            lyricsVersion: response.lyricsVersion,
            lines: lines,
            error: nil
        )
    }

    private func failure(_ message: String) -> LyricSnapshot {
        var snapshot = LyricSnapshot()
        snapshot.error = message
        return snapshot
    }

    private func timeInterval(_ value: String) -> Double? {
        let dayParts = value.split(separator: ".", maxSplits: 1, omittingEmptySubsequences: false)
        let hasDays = dayParts.count == 2 && dayParts[0].allSatisfy(\.isNumber) && dayParts[1].contains(":")
        let clock = hasDays ? String(dayParts[1]) : value
        let days = hasDays ? Double(dayParts[0]) ?? 0 : 0
        let fields = clock.split(separator: ":", omittingEmptySubsequences: false)
        guard fields.count == 2 || fields.count == 3, let last = fields.last.flatMap({ Double($0) }) else { return nil }
        var total = last
        if fields.count >= 2, let minutes = Double(fields[fields.count - 2]) { total += minutes * 60 }
        if fields.count == 3, let hours = Double(fields[0]) { total += hours * 3_600 }
        total += days * 86_400
        return total.isFinite ? total : nil
    }
}
