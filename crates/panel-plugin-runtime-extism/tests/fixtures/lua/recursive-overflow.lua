local function recurse(value)
    return value + recurse(value + 1)
end

function metadata(request)
    return recurse(0)
end
