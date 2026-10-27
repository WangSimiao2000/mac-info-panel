function metadata(request)
    assert(collectgarbage == nil)
    assert(coroutine == nil)
    assert(debug == nil)
    assert(dofile == nil)
    assert(getmetatable == nil)
    assert(host == nil)
    assert(io == nil)
    assert(load == nil)
    assert(loadfile == nil)
    assert(os == nil)
    assert(package == nil)
    assert(rawget == nil)
    assert(rawset == nil)
    assert(require == nil)
    assert(setmetatable == nil)
    assert(table.unpack == nil)
    return {
        id = "io.github.example.restricted",
        version = "0.1.0",
        apiVersion = { major = 1, minor = 0 },
        runtime = "lua@1",
        pages = {
            { id = "restricted", title = "Restricted" },
        },
    }
end
