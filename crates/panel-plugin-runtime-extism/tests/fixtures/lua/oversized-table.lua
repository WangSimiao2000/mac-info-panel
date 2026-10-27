function metadata(request)
    local pages = {}
    for index = 1, 4096 do
        pages[index] = { id = "page", title = "Page" }
    end
    return {
        id = "io.github.example.oversized",
        version = "0.1.0",
        apiVersion = { major = 1, minor = 0 },
        runtime = "lua@1",
        pages = pages,
    }
end
