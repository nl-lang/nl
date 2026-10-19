-- Pattern support, for the tests the compiler's `pattern`
-- module emits.
--
-- Intrinsics: rt.matcherror, rt.istable, rt.inrange, rt.rest.
do
  -- A must-match pattern failed.
  function rt.matcherror(what)
    I.throw("MatchError", "the value does not match: expected " .. tostring(what), 2)
  end

  function rt.istable(v)
    return type(v) == "table"
  end

  -- `a..b` and `a...b` as patterns: a number in the range.
  function rt.inrange(v, first, last, exclusive)
    if type(v) ~= "number" or v < first then return false end
    if exclusive then return v < last end
    return v <= last
  end

  -- `[a, *rest]`: the elements from `from` on, as a new table.
  function rt.rest(t, from)
    local out = {}
    for i = from, #t do out[#out + 1] = t[i] end
    return out
  end
end
