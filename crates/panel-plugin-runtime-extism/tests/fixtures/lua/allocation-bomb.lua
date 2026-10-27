function metadata(request)
    local payload = "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"
    for index = 1, 19 do
        payload = payload .. payload
    end
    return {
        id = "io.github.example.allocation",
        version = "0.1.0",
        apiVersion = { major = 1, minor = 0 },
        runtime = "lua@1",
        pages = {
            { id = "allocation", title = payload },
        },
    }
end
