function metadata(request)
    local oversized = "x"
    for index = 1, 14 do
        oversized = oversized .. oversized
    end
    return {
        id = oversized,
        version = "0.1.0",
        apiVersion = { major = 1, minor = 0 },
        runtime = "lua@1",
        pages = {
            { id = "oversized", title = "Oversized" },
        },
    }
end
