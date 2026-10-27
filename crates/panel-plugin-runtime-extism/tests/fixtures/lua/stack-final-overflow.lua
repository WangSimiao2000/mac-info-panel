local function recurse(depth)
    if depth == 0 then
        return { ok = true }
    end
    local result = recurse(depth - 1)
    return result
end

function metadata(request)
    return recurse(16)
end
