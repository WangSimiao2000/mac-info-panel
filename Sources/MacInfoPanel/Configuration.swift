import Foundation

struct PanelConfiguration: Sendable {
    let screenName: String
    let lyricsURL: URL
    let sshHost: String
    let sshUser: String
    let sshKeyPath: String
    let sshCommand: String
    let lyricsRefreshSeconds: Double
    let systemRefreshSeconds: Double
    let remoteRefreshSeconds: Double

    static func load() -> PanelConfiguration {
        let defaults = PanelConfiguration(
            screenName: "Wokyis",
            lyricsURL: URL(string: "http://127.0.0.1:27270/snapshot")!,
            sshHost: "mc.mickeymiao.cn",
            sshUser: "mickeymiao",
            sshKeyPath: NSString(string: "~/.ssh/id_ed25519_mc_panel").expandingTildeInPath,
            sshCommand: "mcops-status-v1",
            lyricsRefreshSeconds: 1,
            systemRefreshSeconds: 3,
            remoteRefreshSeconds: 15
        )

        let path = NSString(string: "~/.config/mac-info-panel/config.json").expandingTildeInPath
        guard
            let data = FileManager.default.contents(atPath: path),
            let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else { return defaults }

        return PanelConfiguration(
            screenName: object["screenName"] as? String ?? defaults.screenName,
            lyricsURL: (object["lyricsURL"] as? String).flatMap(URL.init(string:)) ?? defaults.lyricsURL,
            sshHost: object["sshHost"] as? String ?? defaults.sshHost,
            sshUser: object["sshUser"] as? String ?? defaults.sshUser,
            sshKeyPath: NSString(string: object["sshKeyPath"] as? String ?? defaults.sshKeyPath).expandingTildeInPath,
            sshCommand: object["sshCommand"] as? String ?? defaults.sshCommand,
            lyricsRefreshSeconds: positive(object["lyricsRefreshSeconds"], fallback: defaults.lyricsRefreshSeconds),
            systemRefreshSeconds: positive(object["systemRefreshSeconds"], fallback: defaults.systemRefreshSeconds),
            remoteRefreshSeconds: positive(object["remoteRefreshSeconds"], fallback: defaults.remoteRefreshSeconds)
        )
    }

    private static func positive(_ value: Any?, fallback: Double) -> Double {
        guard let number = value as? NSNumber, number.doubleValue > 0 else { return fallback }
        return number.doubleValue
    }
}
