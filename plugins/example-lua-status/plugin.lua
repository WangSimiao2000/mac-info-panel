function metadata(request)
    return {
        id = "io.github.wangsimiao.example-lua-status",
        version = "0.1.0",
        apiVersion = { major = 1, minor = 0 },
        runtime = "lua@1",
        pages = {
            {
                id = "example.lua.status",
                title = "Example Lua Status",
                icon = "gauge",
            },
        },
    }
end

function validate_config(request)
    return { valid = true }
end

function refresh(request)
    return {
        page = {
            schemaVersion = 1,
            id = "example.lua.status",
            title = "Example Lua Status",
            icon = "gauge",
            sections = {
                {
                    title = "Lua-in-WASM runtime",
                    widgets = {
                        {
                            kind = "status",
                            label = "Sandbox",
                            value = "Active",
                            tone = "success",
                        },
                        {
                            kind = "metric",
                            label = "Platform",
                            value = request.platform or "Unavailable",
                            unit = json_null,
                        },
                        {
                            kind = "gauge",
                            label = "Example load",
                            value = 0.42,
                        },
                    },
                },
            },
            updatedAtUnixMs = request.nowUnixMs,
        },
    }
end
