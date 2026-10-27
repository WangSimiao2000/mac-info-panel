import Foundation

struct PanelConfiguration: Codable, Sendable {
    struct Display: Codable, Sendable {
        var nameContains: String?
        var vendorID: UInt32?
        var modelID: UInt32?
        var padding: Double
    }

    struct Behavior: Codable, Sendable {
        var initialPage: String
        var autoRotateSeconds: Double
    }

    struct Appearance: Codable, Sendable {
        enum LyricFontDesign: String, Codable, Sendable {
            case system
            case rounded
            case serif
            case monospaced
        }

        var lyricFontDesign: LyricFontDesign
        var lyricFontScale: Double

        static let appleRecommended = Appearance(
            lyricFontDesign: .system,
            lyricFontScale: 1
        )
    }

    struct Lyrics: Codable, Sendable {
        var enabled: Bool
        var mediaControlPath: String?
        var provider: String

        private enum CodingKeys: String, CodingKey {
            case enabled
            case mediaControlPath
            case provider
        }

        init(enabled: Bool, mediaControlPath: String?, provider: String) {
            self.enabled = enabled
            self.mediaControlPath = mediaControlPath
            self.provider = provider
        }

        init(from decoder: Decoder) throws {
            let container = try decoder.container(keyedBy: CodingKeys.self)
            enabled = try container.decodeIfPresent(Bool.self, forKey: .enabled) ?? true
            mediaControlPath = try container.decodeIfPresent(String.self, forKey: .mediaControlPath)
            provider = try container.decodeIfPresent(String.self, forKey: .provider) ?? "netease"
        }
    }

    struct Volume: Codable, Sendable, Identifiable {
        var id: String
        var label: String
        var path: String
    }

    struct Page: Codable, Sendable, Identifiable {
        enum Kind: String, Codable, Sendable {
            case overview
            case lyrics
            case httpJSON
            case localSystem
        }

        var id: String
        var title: String
        var kind: Kind
        var sourceID: String?
    }

    struct HTTPJSONSource: Codable, Sendable, Identifiable {
        struct Field: Codable, Sendable, Identifiable {
            enum ValueType: String, Codable, Sendable {
                case string
                case number
                case boolean
                case percent
                case bytes
                case duration
                case date
            }

            enum Style: String, Codable, Sendable {
                case value
                case status
                case gauge
            }

            var id: String
            var label: String
            var pointer: String
            var type: ValueType
            var style: Style
            var trueText: String?
            var falseText: String?
            var unit: String?
        }

        var id: String
        var title: String
        var symbol: String
        var url: URL
        var refreshSeconds: Double
        var staleAfterSeconds: Double
        var fields: [Field]
    }

    var schemaVersion: Int
    var display: Display
    var behavior: Behavior
    var appearance: Appearance?
    var lyrics: Lyrics
    var pages: [Page]
    var sources: [HTTPJSONSource]
    var volumes: [Volume]

    static let fallback = PanelConfiguration(
        schemaVersion: 1,
        display: .init(nameContains: nil, vendorID: nil, modelID: nil, padding: 10),
        behavior: .init(initialPage: "overview", autoRotateSeconds: 20),
        appearance: .appleRecommended,
        lyrics: .init(
            enabled: true,
            mediaControlPath: nil,
            provider: "netease"
        ),
        pages: [
            .init(id: "overview", title: "总览", kind: .overview, sourceID: nil),
            .init(id: "lyrics", title: "歌词", kind: .lyrics, sourceID: nil),
            .init(id: "local", title: "这台 Mac", kind: .localSystem, sourceID: nil),
        ],
        sources: [],
        volumes: []
    )

    static func load() -> (configuration: PanelConfiguration, warning: String?) {
        let directory = NSString(string: "~/.config/mac-info-panel").expandingTildeInPath
        let candidates = ["config.jsonc", "config.json"].map { (directory as NSString).appendingPathComponent($0) }
        guard let path = candidates.first(where: FileManager.default.fileExists(atPath:)) else {
            return (fallback, "未找到配置文件，正在使用通用默认配置")
        }

        do {
            let raw = try Data(contentsOf: URL(fileURLWithPath: path))
            let data = try JSONComments.strip(raw)
            var configuration = try JSONDecoder().decode(PanelConfiguration.self, from: data)
            try configuration.validate()
            configuration.display.padding = max(0, min(40, configuration.display.padding))
            configuration.behavior.autoRotateSeconds = clampedInterval(configuration.behavior.autoRotateSeconds, minimum: 0)
            if configuration.appearance == nil { configuration.appearance = .appleRecommended }
            var appearance = configuration.appearance ?? .appleRecommended
            appearance.lyricFontScale = max(0.8, min(1.35, appearance.lyricFontScale))
            configuration.appearance = appearance
            for index in configuration.sources.indices {
                configuration.sources[index].refreshSeconds = clampedInterval(configuration.sources[index].refreshSeconds, minimum: 5)
                configuration.sources[index].staleAfterSeconds = max(
                    configuration.sources[index].refreshSeconds * 2,
                    configuration.sources[index].staleAfterSeconds
                )
            }
            return (configuration, nil)
        } catch {
            return (fallback, "配置无效：\(error.localizedDescription)")
        }
    }

    private mutating func validate() throws {
        guard schemaVersion == 1 else { throw ConfigurationError("不支持 schemaVersion \(schemaVersion)") }
        guard !pages.isEmpty else { throw ConfigurationError("pages 不能为空") }
        try unique(pages.map(\.id), name: "page id")
        try unique(sources.map(\.id), name: "source id")
        try unique(volumes.map(\.id), name: "volume id")
        guard ["netease"].contains(lyrics.provider) else {
            throw ConfigurationError("不支持歌词 provider：\(lyrics.provider)")
        }

        let sourceIDs = Set(sources.map(\.id))
        for page in pages where page.kind == .httpJSON {
            guard let sourceID = page.sourceID, sourceIDs.contains(sourceID) else {
                throw ConfigurationError("页面 \(page.id) 引用了不存在的数据源")
            }
        }
        for source in sources {
            try HTTPJSONClient.validateURL(source.url)
            guard !source.fields.isEmpty else { throw ConfigurationError("数据源 \(source.id) 没有字段") }
            try unique(source.fields.map(\.id), name: "field id in \(source.id)")
            for field in source.fields where !field.pointer.isEmpty && !field.pointer.hasPrefix("/") {
                throw ConfigurationError("JSON Pointer 必须为空或以 / 开头：\(field.pointer)")
            }
        }
    }

    private static func clampedInterval(_ value: Double, minimum: Double) -> Double {
        guard value.isFinite else { return max(minimum, 1) }
        if value == 0 && minimum == 0 { return 0 }
        return max(minimum, min(3_600, value))
    }

    private func unique(_ values: [String], name: String) throws {
        guard Set(values).count == values.count else { throw ConfigurationError("存在重复的 \(name)") }
        guard values.allSatisfy({ !$0.isEmpty }) else { throw ConfigurationError("\(name) 不能为空") }
    }
}

private struct ConfigurationError: LocalizedError {
    let message: String
    init(_ message: String) { self.message = message }
    var errorDescription: String? { message }
}

private enum JSONComments {
    static func strip(_ data: Data) throws -> Data {
        let bytes = Array(data)
        var output = Data()
        output.reserveCapacity(bytes.count)
        var index = 0
        var inString = false
        var escaped = false

        while index < bytes.count {
            let byte = bytes[index]
            if inString {
                output.append(byte)
                if escaped { escaped = false }
                else if byte == 0x5C { escaped = true }
                else if byte == 0x22 { inString = false }
                index += 1
                continue
            }

            if byte == 0x22 {
                inString = true
                output.append(byte)
                index += 1
                continue
            }

            if byte == 0x2F, index + 1 < bytes.count {
                let next = bytes[index + 1]
                if next == 0x2F {
                    index += 2
                    while index < bytes.count, bytes[index] != 0x0A, bytes[index] != 0x0D { index += 1 }
                    continue
                }
                if next == 0x2A {
                    index += 2
                    var closed = false
                    while index + 1 < bytes.count {
                        if bytes[index] == 0x0A || bytes[index] == 0x0D { output.append(bytes[index]) }
                        if bytes[index] == 0x2A, bytes[index + 1] == 0x2F {
                            index += 2
                            closed = true
                            break
                        }
                        index += 1
                    }
                    guard closed else { throw ConfigurationError("JSONC 块注释没有结束") }
                    continue
                }
            }

            output.append(byte)
            index += 1
        }
        return output
    }
}
