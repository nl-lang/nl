-- NL's own Number methods, in Lua's `math` table with Lua's functions:
-- `math` is NL's `Number` and the `__index` of numbers (nl_90_prelude.lua),
-- so `x.clamp(0, 1)` in NL and `x:clamp(0, 1)` in Lua both reach them, and
-- `Number.lerp(a, b, t)` is `a.lerp(b, t)`. The names are lowercase and
-- none is one of Lua's; Garry's Mod's own (`math.Clamp`, `math.Round`, …)
-- are PascalCase and stay as they are, with their own semantics.
--
-- These are called in hot code, so the arithmetic ones take numbers as
-- they come: another value raises Lua's own arithmetic error. An integer
-- argument (places, a base, a count) is checked.
--
-- Choices where the language is silent, marked "Unspecified" at each
-- function: rounding halfway cases, angles, `times` counting from 1,
-- `divisible_by(0)`, `even`/`odd` of a non-integer, `to_s` without a base.
do
  local floor, ceil, abs, sqrt, frexp = math.floor, math.ceil, math.abs, math.sqrt, math.frexp
  local huge = math.huge
  local int_arg = I.int_arg

  local function int(v, fname, what, default)
    return int_arg(v, fname, what, default, "Number")
  end

  -- Unspecified: lo greater than hi raises, as in Ruby.
  function math.clamp(x, lo, hi)
    if lo > hi then
      I.throw("Exception", "Number.clamp: the minimum " .. tostring(lo) .. " is greater than the maximum " .. tostring(hi), 2)
    end
    if x < lo then return lo end
    if x > hi then return hi end
    return x
  end

  -- Half away from zero, to an integer.
  local function round0(x)
    if x >= 0 then
      local f = floor(x)
      if x - f >= 0.5 then return f + 1 end
      return f
    end
    local f = ceil(x)
    if f - x >= 0.5 then return f - 1 end
    return f
  end

  -- Whether `places` digits after the point keep `x` as it is (all its
  -- digits are before them), or make it 0; Ruby's float_round_overflow and
  -- float_round_underflow, from the binary exponent of x.
  local function keeps(x, places)
    local _, e = frexp(x)
    local q = e > 0 and floor(e / 4) or -floor(-e / 3) - 1
    return places >= 17 - q
  end
  local function vanishes(x, places)
    local _, e = frexp(x)
    local q = e > 0 and floor(e / 3) + 1 or -floor(-e / 4)
    return places < -q
  end

  -- x scaled for `places` digits, the scale, and what to divide the
  -- result by after. Past 308 places 10^places is inf, and an x that
  -- `keeps` has not kept is tiny (below 1e-263): it is multiplied by
  -- 10^300 first, which leaves it a normal number, and the scale is the
  -- rest of 10^places.
  local function prescale(x, places)
    if places > 308 then return x * 1e300, 10 ^ (places - 300), 1e300 end
    return x, 10 ^ places, 1
  end

  -- `x.round(places = 0)`: to `places` digits after the point (before it
  -- when negative). Unspecified: halfway cases go away from zero (Ruby's
  -- default, not Lua's or C's banker's rounding), and a decimal halfway
  -- case is judged on the decimal value, not its nearest double, as Ruby
  -- does: 2.675.round(2) is 2.68.
  function math.round(x, places)
    if x ~= x or x == huge or x == -huge then return x end
    places = int(places, "round", 1, 0)
    if places == 0 then return round0(x) end
    if places < 0 then
      local s = 10 ^ -places
      return round0(x / s) * s
    end
    if keeps(x, places) then return x end
    if x ~= 0 and vanishes(x, places) then return 0 end
    local s, post
    x, s, post = prescale(x, places)
    local f = round0(x * s)
    if x > 0 then
      if (f + 0.5) / s <= x then f = f + 1 end
    elseif x < 0 then
      if (f - 0.5) / s >= x then f = f - 1 end
    end
    return f / s / post
  end

  -- `x.truncate(places = 0)`: toward zero, to `places` digits (Ruby's,
  -- with the same decimal judgement as round: 0.29.truncate(2) is 0.29).
  local function trunc_pos(x, places)
    if places <= 0 then
      local i = floor(x)
      if places == 0 then return i end
      local s = 10 ^ -places
      return floor(i / s) * s
    end
    if keeps(x, places) then return x end
    if vanishes(x, places) then return 0 end
    local s, post
    x, s, post = prescale(x, places)
    local m = floor(x * s)
    local r = (m + 1) / s
    if r > x then r = m / s end
    return r / post
  end

  function math.truncate(x, places)
    if x ~= x or x == huge or x == -huge then return x end
    places = int(places, "truncate", 1, 0)
    if x < 0 then return -trunc_pos(-x, places) end
    return trunc_pos(x, places)
  end

  -- -1, 0 or 1; NaN stays NaN.
  function math.sign(x)
    if x > 0 then return 1 end
    if x < 0 then return -1 end
    if x == 0 then return 0 end
    return x
  end

  function math.lerp(a, b, t)
    return a + (b - a) * t
  end

  -- `x.remap(a1, b1, a2, b2)`: x's place between a1 and b1, put between
  -- a2 and b2, not clamped (Garry's Mod's math.Remap).
  function math.remap(x, a1, b1, a2, b2)
    return a2 + (x - a1) * (b2 - a2) / (b1 - a1)
  end

  -- `x.approach(target, step)`: `step` (its size) closer to target,
  -- without passing it.
  local function approach(x, target, step)
    step = abs(step)
    if x < target then
      x = x + step
      if x > target then return target end
      return x
    elseif x > target then
      x = x - step
      if x < target then return target end
      return x
    end
    return target
  end
  math.approach = approach

  -- Degrees. Unspecified: an angle is normalized to [-180, 180), so 180
  -- is -180 (Garry's Mod's math.NormalizeAngle).
  local function normalize(a)
    return (a + 180) % 360 - 180
  end
  math.normalize_angle = normalize

  -- `a.angle_difference(b)`: the shortest turn from b to a, normalized.
  function math.angle_difference(a, b)
    return normalize(a - b)
  end

  -- `a.approach_angle(target, step)`: `step` degrees closer to target, the
  -- short way round; the result is not normalized.
  function math.approach_angle(a, target, step)
    return approach(a, a + normalize(target - a), step)
  end

  -- Unspecified: a number that is not an integer is neither even nor odd.
  function math.even(x)
    return x % 2 == 0
  end

  function math.odd(x)
    return x % 2 == 1
  end

  -- Unspecified: nothing is divisible by 0 (x % 0 is NaN).
  function math.divisible_by(x, n)
    return x % n == 0
  end

  -- Both ends included.
  function math.between(x, a, b)
    return x >= a and x <= b
  end

  function math.is_integer(x)
    return x == floor(x) and x ~= huge and x ~= -huge
  end

  function math.is_nan(x)
    return x ~= x
  end

  function math.is_finite(x)
    return x == x and x ~= huge and x ~= -huge
  end

  -- sqrt(x² + y²) without overflow in the squares.
  function math.hypot(x, y)
    x, y = abs(x), abs(y)
    if x < y then x, y = y, x end
    if x == huge then return huge end
    if x == 0 or y ~= y then return x + y end
    local r = y / x
    return x * sqrt(1 + r * r)
  end

  local digits = "0123456789abcdefghijklmnopqrstuvwxyz"

  -- `x.to_s(base)`: the digits of an integer in a base from 2 to 36,
  -- lowercase, with a "-" for a negative one. Unspecified: without a base,
  -- Lua's tostring (`%.14g`), for any number.
  local function to_s(x, base, fname)
    if base == nil then return tostring(x) end
    base = int(base, fname, 1)
    if base < 2 or base > 36 then
      I.throw("TypeError", "Number." .. fname .. " argument 1 expects a base from 2 to 36, got " .. tostring(base), 3)
    end
    if type(x) ~= "number" or x ~= floor(x) or x == huge or x == -huge then
      I.throw("TypeError", "Number." .. fname .. " expects an integer, got "
        .. (type(x) == "number" and tostring(x) or rt.typeof(x)), 3)
    end
    local neg = x < 0
    if neg then x = -x end
    local out, k = {}, 0
    repeat
      local d = x % base
      k = k + 1
      out[k] = lstring.sub(digits, d + 1, d + 1)
      x = (x - d) / base
    until x == 0
    if neg then
      k = k + 1
      out[k] = "-"
    end
    return lstring.reverse(table.concat(out))
  end

  function math.to_s(x, base)
    return to_s(x, base, "to_s")
  end

  -- `x.to_hex(width)`: to_s(16), with zeros before the digits up to
  -- `width` digits.
  function math.to_hex(x, width)
    local s = to_s(x, 16, "to_hex")
    width = int(width, "to_hex", 1, 0)
    local neg = lstring.sub(s, 1, 1) == "-"
    local d = neg and lstring.sub(s, 2) or s
    if #d < width then d = lstring.rep("0", width - #d) .. d end
    return neg and ("-" .. d) or d
  end

  -- `n.times(f)`: f(1), …, f(n). Unspecified: the count starts at 1, as
  -- positions do in NL (Ruby's starts at 0).
  function math.times(n, f)
    n = int(n, "times", "receiver")
    for i = 1, n do f(i) end
  end

  -- `a.up_to(b, f)`: f(a), f(a + 1), …, f(b); none when b < a.
  function math.up_to(a, b, f)
    for i = a, b do f(i) end
  end

  -- `a.down_to(b, f)`: f(a), f(a - 1), …, f(b); none when b > a.
  function math.down_to(a, b, f)
    for i = a, b, -1 do f(i) end
  end
end
