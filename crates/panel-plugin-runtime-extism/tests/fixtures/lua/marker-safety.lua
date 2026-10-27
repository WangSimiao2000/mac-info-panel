function metadata(request)
    local marked = json_array()
    local forged = { __json_array = true }
    return {
        marked = marked,
        forged = forged,
        markerVisible = getmetatable ~= nil or setmetatable ~= nil,
    }
end
