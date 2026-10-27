import Foundation

actor NeteaseLyricsProvider {
    enum ProviderError: LocalizedError {
        case noMatch
        case noLyrics
        case invalidResponse
        case responseTooLarge

        var errorDescription: String? {
            switch self {
            case .noMatch: return "没有找到匹配歌曲"
            case .noLyrics: return "没有找到同步歌词"
            case .invalidResponse: return "歌词服务响应无效"
            case .responseTooLarge: return "歌词服务响应过大"
            }
        }
    }

    private struct SearchResponse: Decodable {
        struct Result: Decodable { let songs: [Song]? }
        let code: Int
        let result: Result?
    }

    private struct Song: Decodable {
        struct Artist: Decodable { let name: String }
        struct Album: Decodable { let name: String }
        let id: Int64
        let name: String
        let artists: [Artist]
        let album: Album?
        let duration: Double?
    }

    private struct LyricsResponse: Decodable {
        struct Lyrics: Decodable { let lyric: String? }
        let code: Int
        let nolyric: Bool?
        let lrc: Lyrics?
        let tlyric: Lyrics?
    }

    private struct CacheEntry {
        let lines: [LyricLine]
        let expiresAt: Date
    }

    private var cache: [String: CacheEntry] = [:]

    func lyrics(for track: PlaybackState) async throws -> [LyricLine] {
        let key = track.trackKey
        if let cached = cache[key], cached.expiresAt > Date() {
            if cached.lines.isEmpty { throw ProviderError.noLyrics }
            return cached.lines
        }

        do {
            let song = try await search(track)
            let lines = try await fetchLyrics(songID: song.id)
            cache[key] = CacheEntry(lines: lines, expiresAt: Date().addingTimeInterval(7 * 86_400))
            return lines
        } catch {
            if Task.isCancelled || (error as? URLError)?.code == .cancelled {
                throw CancellationError()
            }
            cache[key] = CacheEntry(lines: [], expiresAt: Date().addingTimeInterval(60))
            throw error
        }
    }

    private func search(_ track: PlaybackState) async throws -> Song {
        var components = URLComponents(string: "https://music.163.com/api/search/get/web")!
        components.queryItems = [
            URLQueryItem(name: "s", value: [track.title, track.artist].filter { !$0.isEmpty }.joined(separator: " ")),
            URLQueryItem(name: "type", value: "1"),
            URLQueryItem(name: "offset", value: "0"),
            URLQueryItem(name: "total", value: "true"),
            URLQueryItem(name: "limit", value: "20"),
        ]
        let data = try await request(components.url!, limit: 1_048_576)
        let response = try JSONDecoder().decode(SearchResponse.self, from: data)
        guard response.code == 200 else { throw ProviderError.invalidResponse }
        let songs = response.result?.songs ?? []
        let expectedTitle = normalize(track.title)
        let expectedArtists = artistParts(track.artist)
        let tolerance = min(6, max(3, track.duration * 0.02))

        let matches = songs.filter { song in
            guard normalize(song.name) == expectedTitle else { return false }
            let candidateArtists = Set(song.artists.flatMap { artistParts($0.name) })
            guard expectedArtists.isEmpty || !candidateArtists.isDisjoint(with: expectedArtists) else { return false }
            guard let milliseconds = song.duration, track.duration > 0 else { return true }
            return abs(milliseconds / 1_000 - track.duration) <= tolerance
        }
        guard let best = matches.min(by: { durationDifference($0, track) < durationDifference($1, track) }) else {
            throw ProviderError.noMatch
        }
        return best
    }

    private func fetchLyrics(songID: Int64) async throws -> [LyricLine] {
        var components = URLComponents(string: "https://music.163.com/api/song/lyric")!
        components.queryItems = [
            URLQueryItem(name: "id", value: String(songID)),
            URLQueryItem(name: "lv", value: "-1"),
            URLQueryItem(name: "kv", value: "-1"),
            URLQueryItem(name: "tv", value: "-1"),
        ]
        let data = try await request(components.url!, limit: 4_194_304)
        let response = try JSONDecoder().decode(LyricsResponse.self, from: data)
        guard response.code == 200 else { throw ProviderError.invalidResponse }
        guard response.nolyric != true, let raw = response.lrc?.lyric, !raw.isEmpty else {
            throw ProviderError.noLyrics
        }
        let original = parseLRC(raw)
        guard !original.isEmpty else { throw ProviderError.noLyrics }
        let translated = parseLRC(response.tlyric?.lyric ?? "")
        return attachTranslations(original, translated)
    }

    private func request(_ url: URL, limit: Int) async throws -> Data {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.timeoutIntervalForRequest = 8
        configuration.timeoutIntervalForResource = 12
        configuration.httpCookieStorage = nil
        configuration.urlCache = nil
        let session = URLSession(configuration: configuration)
        defer { session.finishTasksAndInvalidate() }

        var request = URLRequest(url: url)
        request.setValue("Mozilla/5.0 (Macintosh; Intel Mac OS X) AppleWebKit/537.36 Safari/537.36", forHTTPHeaderField: "User-Agent")
        request.setValue("https://music.163.com/", forHTTPHeaderField: "Referer")
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        let (bytes, response) = try await session.bytes(for: request)
        guard let http = response as? HTTPURLResponse, http.statusCode == 200 else { throw ProviderError.invalidResponse }
        if http.expectedContentLength > Int64(limit) { throw ProviderError.responseTooLarge }
        var data = Data()
        data.reserveCapacity(min(limit, max(0, Int(http.expectedContentLength))))
        for try await byte in bytes {
            guard data.count < limit else { throw ProviderError.responseTooLarge }
            data.append(byte)
        }
        return data
    }

    private func parseLRC(_ raw: String) -> [LyricLine] {
        let pattern = #"\[(\d+):(\d{2})(?:[\.:](\d{1,3}))?\]"#
        guard let regex = try? NSRegularExpression(pattern: pattern) else { return [] }
        var parsed: [(Double, String)] = []
        for sourceLine in raw.components(separatedBy: .newlines) {
            let range = NSRange(sourceLine.startIndex..., in: sourceLine)
            let matches = regex.matches(in: sourceLine, range: range)
            guard !matches.isEmpty else { continue }
            let textStart = matches.last!.range.location + matches.last!.range.length
            guard let textRange = Range(NSRange(location: textStart, length: max(0, range.length - textStart)), in: sourceLine) else { continue }
            let text = sourceLine[textRange].trimmingCharacters(in: .whitespaces)
            guard !text.isEmpty else { continue }
            for match in matches {
                guard
                    let minuteRange = Range(match.range(at: 1), in: sourceLine),
                    let secondRange = Range(match.range(at: 2), in: sourceLine),
                    let minutes = Double(sourceLine[minuteRange]),
                    let seconds = Double(sourceLine[secondRange])
                else { continue }
                var fraction = 0.0
                if match.range(at: 3).location != NSNotFound, let fractionRange = Range(match.range(at: 3), in: sourceLine) {
                    fraction = Double("0." + sourceLine[fractionRange]) ?? 0
                }
                parsed.append((minutes * 60 + seconds + fraction, text))
            }
        }
        let sorted = parsed.sorted { $0.0 < $1.0 }
        return sorted.enumerated().map { index, item in
            LyricLine(
                id: index,
                startTime: item.0,
                endTime: index + 1 < sorted.count ? sorted[index + 1].0 : nil,
                text: item.1,
                translation: "",
                tokens: []
            )
        }
    }

    private func attachTranslations(_ original: [LyricLine], _ translations: [LyricLine]) -> [LyricLine] {
        var used = Set<Int>()
        return original.map { line in
            let candidate = translations
                .filter { !used.contains($0.id) && abs($0.startTime - line.startTime) <= 0.35 }
                .min { abs($0.startTime - line.startTime) < abs($1.startTime - line.startTime) }
            if let candidate { used.insert(candidate.id) }
            return LyricLine(
                id: line.id,
                startTime: line.startTime,
                endTime: line.endTime,
                text: line.text,
                translation: candidate?.text == line.text ? "" : candidate?.text ?? "",
                tokens: line.tokens
            )
        }
    }

    private func durationDifference(_ song: Song, _ track: PlaybackState) -> Double {
        guard let duration = song.duration else { return .greatestFiniteMagnitude }
        return abs(duration / 1_000 - track.duration)
    }

    private func normalize(_ value: String) -> String {
        value
            .folding(options: [.caseInsensitive, .diacriticInsensitive, .widthInsensitive], locale: .current)
            .unicodeScalars
            .filter { CharacterSet.alphanumerics.contains($0) }
            .map(String.init)
            .joined()
    }

    private func artistParts(_ value: String) -> Set<String> {
        let separators = CharacterSet(charactersIn: ",，、&;/；")
        return Set(value.components(separatedBy: separators).map(normalize).filter { !$0.isEmpty })
    }
}
