function metadata(request)
    local first = math.random()
    local second = math.random()
    math.randomseed()
    local reset = math.random()
    return {
        first = first,
        second = second,
        reset = reset,
    }
end
