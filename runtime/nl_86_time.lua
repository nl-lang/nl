-- NL's `Time` and `Duration`, the built-in types of a point in time and of
-- a length of time.
--
-- A Duration is an immutable number of seconds (any finite number,
-- fractions and negatives included). A Time is an immutable number of
-- seconds since the Unix epoch (1970-01-01T00:00:00Z), possibly
-- fractional, and a view: local or UTC. No time zone is stored; the view
-- says how the calendar (the accessors, `wday`, `yday`, `iso`, `to_s`,
-- `strftime`, `date`, `clock`, `beginning_of_day`) is read: in local time,
-- or in UTC for the Time `t.utc` gives (`t.local` gives the local one
-- back, `t.is_utc` says which). The keyed argument `utc: true` (or
-- `utc: false`) reads one call in UTC (or local time) whatever the view;
-- `Time.new` and `Time.parse` take it too, for reading their components.
-- Every constructor (`Time.at`, `Time.new`, `Time.parse`, `Time.now`,
-- `ago`, `from_now`) gives a local view, and what is computed from a
-- Time (`+`, `-`, `floor_to`, `beginning_of_day`) keeps its view. The
-- view is not part of the value: `==`, `<` and `since` compare epochs
-- only, so `t.utc == t`. A view is the value's index 2 (true for UTC), so
-- it crosses to workers with the epoch.
--
-- Each type is a table that is three things at once, as `Regex` is: the
-- library (`Time.at`, `Duration.minutes`), the metatable of its values
-- (the operators are its metamethods, which NL's dispatch reaches as
-- metamethods written in Lua: nl_40_ops.lua, `perform`), and the type for
-- `is`, `typeof` and checks (I.info kind "builtin": a value is one when
-- its metatable is the type). Instances find their methods in a separate
-- table, marked as methods so that `t.year` calls (nl_30_member.lua), and
-- the library functions are not reachable through an instance: the
-- library's `Duration.minutes(5)` constructs, while an instance's parts
-- are `d.parts`. An instance holds its number at index 1 and refuses new
-- keys.
--
-- Why not structs: a struct can be constructed with a literal, extended,
-- and its fields assigned, none of which a value type should allow, and
-- the analyzer would need the struct's operators spelled out in Rust. As
-- declared library types (decl/nl.d.nl; prelude names with Lua-class
-- types in the analyzer) they get typed members and operators from the
-- declaration file alone, and `t.year` on a value known to be a Time
-- compiles to a plain Lua method call.
--
-- The calendar is computed here, in Lua, for every date (the proleptic
-- Gregorian calendar of days_from_civil and civil_from_days below, H.
-- Hinnant's algorithms), so UTC needs nothing from the host and gives the
-- same answer on every platform. Local time needs the host's time zone
-- rules, which Lua offers only as `os.date("*t", t)`: the local offset at
-- an instant is the difference between those fields read as UTC and the
-- instant. `Time.new` in local time asks `os.time` (mktime, which settles
-- DST), and without it searches with the offset. `strftime` is this
-- file's own (the C locale's names), not `os.date`: it handles fractional
-- times, gives one result on every platform, and needs no `os`.
--
-- What the host must have. `os.time` and `os.date` are taken when the
-- chunk opens; Garry's Mod clients keep both. Without `os.date`, local
-- time is UTC. `Time.now` reads `host.now()` when the host gives one (a
-- clock in seconds since the epoch, possibly fractional), else
-- `os.time()`, in whole seconds (`os.clock` is CPU time, not a clock);
-- with neither it raises, and everything else still works.
--
-- Choices where the language is silent, marked "unspecified":
--   local time where the host cannot give it (no `os.date`, or one that
--       fails for the instant, as Windows does before 1970): UTC;
--   rounding: the calendar, `iso` and `strftime` read a Time rounded to
--       the millisecond, and Duration's `parts`, `to_s` and `iso` a
--       duration rounded to the millisecond;
--   Time.new: components out of range carry over, as `os.time` does
--       (month 13 is January of the next year, day 0 the last of the
--       month before); all but `sec` must be integers;
--   Time.parse: the ISO 8601 extended forms only (see there), strict,
--       raising ConversionError for anything else;
--   `wday`: 0 is Sunday (Ruby's), `yday`: 1 is January 1;
--   `floor_to(step)`: counts steps from the epoch, so in UTC;
--   `humanize`: the buckets of Rails' distance_of_time_in_words, with
--       seconds below 45 counted (see `humanize`), a month of 30 days and
--       a year of 365;
--   operands of the wrong type raise TypeError; `Duration + Time` is one
--       (`Time + Duration` is not);
--   NaN and ±inf are not times or durations: a constructor or an
--       operator given one, and a Duration divided by 0 or by a zero
--       Duration, raise TypeError;
--   the range of a Time: an epoch below 1e15 seconds either way (about
--       31.7 million years), where the calendar's arithmetic is exact to
--       the second; of a Duration: below 2e15 seconds either way (the
--       distance between any two Times). A value beyond, made or
--       computed, raises ConversionError.
do
  local abs = math.abs
  local KW = I.KW

  local los = type(os) == "table" and os or nil
  local os_time = los and type(los.time) == "function" and los.time or nil
  local os_date = los and type(los.date) == "function" and los.date or nil
  local host_now = type(host) == "table" and type(host.now) == "function" and host.now or nil

  local Time, Duration = {}, {}
  local TimeM, DurationM = {}, {}
  rt.Time, rt.Duration = Time, Duration
  I.Time, I.Duration = Time, Duration
  for T, name in pairs({ [Time] = "Time", [Duration] = "Duration" }) do
    I.typeof_special[T] = name
    I.info[T] = { kind = "builtin", name = name }
  end
  Time.__index, Duration.__index = TimeM, DurationM

  local function frozen(v)
    I.throw("TypeError", rt.typeof(v) .. " values cannot be changed", 2)
  end
  Time.__newindex, Duration.__newindex = frozen, frozen

  -- What a value is called in a message: its type, or for a number that
  -- is not finite what it is ("NaN", "inf", "-inf"), as rt.typeof says
  -- Number.
  local function desc(v)
    if type(v) == "number" and v - v ~= 0 then
      if v ~= v then return "NaN" end
      return v > 0 and "inf" or "-inf"
    end
    return rt.typeof(v)
  end

  local LIMIT, DLIMIT = 1e15, 2e15
  local function mkdur(s)
    if not (s > -DLIMIT and s < DLIMIT) then
      I.throw("ConversionError", "the Duration of " .. tostring(s) .. " seconds is out of range (below 2e15 seconds either way)", 3)
    end
    return setmetatable({ s }, Duration)
  end
  -- `utc`: true for a UTC view, nil for local.
  local function mktime(e, utc)
    if not (e > -LIMIT and e < LIMIT) then
      I.throw("ConversionError", "the Time " .. tostring(e) .. " is out of range (an epoch below 1e15 seconds either way)", 3)
    end
    return setmetatable({ e, utc }, Time)
  end
  local function isdur(v) return getmetatable(v) == Duration end
  local function istime(v) return getmetatable(v) == Time end

  -- An operator applied to operands it does not take.
  local function operands(sym, a, b)
    I.throw("TypeError", "cannot apply '" .. sym .. "' to " .. desc(a) .. " and " .. desc(b), 3)
  end

  local function bad(what, expected, v)
    I.throw("TypeError", what .. " expects " .. expected .. ", got " .. desc(v), 3)
  end
  local function num(v, what)
    if type(v) ~= "number" then bad(what, "Number", v) end
    if v - v ~= 0 then bad(what, "a finite Number", v) end
    return v
  end
  local function int(v, what)
    if type(v) ~= "number" or v ~= floor(v) or v == huge or v == -huge then
      I.throw("TypeError", what .. " expects an integer, got " .. (type(v) == "number" and v - v == 0 and tostring(v) or desc(v)), 3)
    end
    return v
  end
  local function recv(v, T, fname)
    if getmetatable(v) ~= T then bad(I.info[T].name .. "." .. fname .. " receiver", I.info[T].name, v) end
    return v[1]
  end

  -- The keyed arguments `utc: Bool?` (nl_40_ops.lua, KW): true for UTC,
  -- false for local time, nil when not given.
  local function utc_opt(kw, fname)
    if kw == nil then return nil end
    if getmetatable(kw) ~= KW then bad(fname .. " keyed arguments", "{utc: Bool?}", kw) end
    for k, v in pairs(kw) do
      if k ~= "utc" then I.throw("TypeError", fname .. " takes no keyed argument '" .. tostring(k) .. "'", 3) end
      if type(v) ~= "boolean" then bad("keyed argument 'utc' of " .. fname, "Bool", v) end
    end
    return kw.utc
  end

  -- Whether a call on Time `t` reads the calendar in UTC: its `utc:`
  -- argument when given, else the view.
  local function utc_of(t, kw, fname)
    local u = utc_opt(kw, fname)
    if u == nil then return t[2] == true end
    return u
  end

  -- Splits a keyed table off the end of `...`: the table (or nil) and the
  -- number of positional arguments.
  local function split_kw(...)
    local n = select("#", ...)
    if n > 0 and getmetatable((select(n, ...))) == KW then return (select(n, ...)), n - 1 end
    return nil, n
  end

  ---------------------------------------------------------------------------
  -- The calendar

  -- Days since 1970-01-01 of a proleptic Gregorian date; month 1 to 12,
  -- any day (it counts on linearly).
  local function days_from_civil(y, m, d)
    if m <= 2 then y = y - 1 end
    local era = floor(y / 400)
    local yoe = y - era * 400
    local mp = (m + 9) % 12
    local doy = floor((153 * mp + 2) / 5) + d - 1
    local doe = yoe * 365 + floor(yoe / 4) - floor(yoe / 100) + doy
    return era * 146097 + doe - 719468
  end

  local function civil_from_days(z)
    z = z + 719468
    local era = floor(z / 146097)
    local doe = z - era * 146097
    local yoe = floor((doe - floor(doe / 1460) + floor(doe / 36524) - floor(doe / 146096)) / 365)
    local doy = doe - (365 * yoe + floor(yoe / 4) - floor(yoe / 100))
    local mp = floor((5 * doy + 2) / 153)
    local d = doy - floor((153 * mp + 2) / 5) + 1
    local m = mp < 10 and mp + 3 or mp - 9
    local y = yoe + era * 400
    if m <= 2 then y = y + 1 end
    return y, m, d
  end

  local function leap(y) return y % 4 == 0 and (y % 100 ~= 0 or y % 400 == 0) end
  local month_days = { 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31 }
  local function days_in_month(y, m)
    if m == 2 and leap(y) then return 29 end
    return month_days[m]
  end

  -- Seconds since the epoch of a UTC date and time; month carries over
  -- into the year, the rest counts on linearly.
  local function utc_epoch(y, mo, d, h, mi, s)
    y = y + floor((mo - 1) / 12)
    mo = (mo - 1) % 12 + 1
    return days_from_civil(y, mo, d) * 86400 + h * 3600 + mi * 60 + s
  end

  -- The local offset from UTC at whole second `e`, in seconds (see the
  -- header); 0 where the host cannot say.
  local function offset_at(e)
    if os_date == nil then return 0 end
    local ok, lt = pcall(os_date, "*t", e)
    if not ok or type(lt) ~= "table" or type(lt.year) ~= "number" then return 0 end
    return utc_epoch(lt.year, lt.month, lt.day, lt.hour, lt.min, lt.sec) - e
  end

  -- The calendar of epoch `e`, rounded to the millisecond, in UTC or local
  -- time: year, month, day, hour, min, sec, ms, days since the epoch (of
  -- the local date), offset in seconds.
  local function fields(e, utc)
    local ems = floor(e * 1000 + 0.5)
    local whole = floor(ems / 1000)
    local ms = ems - whole * 1000
    local off = utc and 0 or offset_at(whole)
    local l = whole + off
    local z = floor(l / 86400)
    local r = l - z * 86400
    local y, mo, d = civil_from_days(z)
    local h = floor(r / 3600)
    r = r - h * 3600
    local mi = floor(r / 60)
    return y, mo, d, h, mi, r - mi * 60, ms, z, off
  end

  ---------------------------------------------------------------------------
  -- Duration

  local function check_dur(v, what)
    if getmetatable(v) ~= Duration then bad(what, "Duration", v) end
    return v[1]
  end

  function Duration.new(seconds)
    return mkdur(num(seconds, "Duration.new argument 1"))
  end
  -- One constructor per unit; the Number helpers (`5.minutes`) are the
  -- same functions, stored in `math` like the other methods of numbers.
  -- Milliseconds are divided, so that `250.ms` is exactly 0.25 seconds.
  local function unit(name, f)
    Duration[name] = f
    math[name] = f
  end
  for name, secs in pairs({ seconds = 1, minutes = 60, hours = 3600, days = 86400, weeks = 604800 }) do
    local what = "Duration." .. name .. " argument 1"
    unit(name, function(n) return mkdur(num(n, what) * secs) end)
  end
  unit("ms", function(n) return mkdur(num(n, "Duration.ms argument 1") / 1000) end)

  function DurationM.total_seconds(d) return recv(d, Duration, "total_seconds") end
  function DurationM.total_ms(d) return recv(d, Duration, "total_ms") * 1000 end
  function DurationM.total_minutes(d) return recv(d, Duration, "total_minutes") / 60 end
  function DurationM.total_hours(d) return recv(d, Duration, "total_hours") / 3600 end
  function DurationM.total_days(d) return recv(d, Duration, "total_days") / 86400 end

  function DurationM.abs(d) return mkdur(abs(recv(d, Duration, "abs"))) end

  -- The parts of `s` seconds, rounded to the millisecond: days, hours,
  -- minutes, seconds, ms, each with the sign of `s` (never -0), and
  -- whether `s` is negative.
  local function split(s)
    local neg = s < 0
    local t = floor(abs(s) * 1000 + 0.5)
    local days = floor(t / 86400000)
    t = t - days * 86400000
    local h = floor(t / 3600000)
    t = t - h * 3600000
    local m = floor(t / 60000)
    t = t - m * 60000
    local sec = floor(t / 1000)
    local ms = t - sec * 1000
    if neg then
      if days ~= 0 then days = -days end
      if h ~= 0 then h = -h end
      if m ~= 0 then m = -m end
      if sec ~= 0 then sec = -sec end
      if ms ~= 0 then ms = -ms end
    end
    return days, h, m, sec, ms, neg
  end

  function DurationM.parts(d)
    local days, h, m, s, ms = split(recv(d, Duration, "parts"))
    return { days = days, hours = h, minutes = m, seconds = s, ms = ms }
  end

  -- "1h 30m", "2d 3h", "1s 500ms", "250ms", "-45s"; "0s" for zero. Parts
  -- that are zero are left out; days are the largest unit.
  function DurationM.to_s(d)
    local days, h, m, s, ms, neg = split(recv(d, Duration, "to_s"))
    local out = {}
    for _, p in ipairs({ { days, "d" }, { h, "h" }, { m, "m" }, { s, "s" }, { ms, "ms" } }) do
      if p[1] ~= 0 then out[#out + 1] = abs(p[1]) .. p[2] end
    end
    if #out == 0 then return "0s" end
    return (neg and "-" or "") .. tconcat(out, " ")
  end
  Duration.__tostring = DurationM.to_s

  -- ISO 8601: "PT1H30M", "P2DT3H", "PT0.25S", "PT0S"; a negative duration
  -- with a leading "-" (ISO 8601-2).
  function DurationM.iso(d)
    local days, h, m, s, ms, neg = split(recv(d, Duration, "iso"))
    days, h, m, s, ms = abs(days), abs(h), abs(m), abs(s), abs(ms)
    local out = (neg and "-" or "") .. "P" .. (days ~= 0 and (days .. "D") or "")
    local tpart = (h ~= 0 and (h .. "H") or "") .. (m ~= 0 and (m .. "M") or "")
    if ms ~= 0 then
      tpart = tpart .. s .. (sgsub(sformat(".%03d", ms), "0+$", "")) .. "S"
    elseif s ~= 0 then
      tpart = tpart .. s .. "S"
    end
    if tpart ~= "" then return out .. "T" .. tpart end
    if days ~= 0 then return out end
    return "PT0S"
  end

  local function plural(n, unit)
    if n == 1 then return "1 " .. unit end
    return n .. " " .. unit .. "s"
  end
  local function round(x) return floor(x + 0.5) end

  -- Words for a length of time, its sign ignored. Rails' buckets
  -- (distance_of_time_in_words), with seconds counted below 45:
  --   below 1s               "less than a second"
  --   1s to 44s              "N seconds" (whole seconds)
  --   45s to 89s             "1 minute"
  --   1m30s to 44m29s        "N minutes" (rounded)
  --   44m30s to 89m29s       "about 1 hour"
  --   89m30s to 23h59m29s    "about N hours" (rounded)
  --   23h59m30s to 41h59m29s "1 day"
  --   to 29d23h59m29s        "N days" (rounded)
  --   to 44d23h59m29s        "about 1 month"
  --   to 59d23h59m29s        "about 2 months"
  --   to 365 days            "N months" (rounded, months of 30 days)
  --   then, in years of 365 days and N whole years: "about N years" up to
  --   3 months past, "over N years" up to 9 months past, else "almost
  --   N+1 years".
  local function words(s)
    s = abs(s)
    if s < 1 then return "less than a second" end
    if s < 45 then return plural(floor(s), "second") end
    if s < 90 then return "1 minute" end
    if s < 2670 then return plural(round(s / 60), "minute") end
    if s < 5370 then return "about 1 hour" end
    if s < 86370 then return "about " .. plural(round(s / 3600), "hour") end
    if s < 151170 then return "1 day" end
    if s < 2591970 then return plural(round(s / 86400), "day") end
    if s < 3887970 then return "about 1 month" end
    if s < 5183970 then return "about 2 months" end
    if s < 31536000 then return plural(math.min(round(s / 2592000), 12), "month") end
    local years = floor(s / 31536000)
    local rest = s - years * 31536000
    if rest < 7776000 then return "about " .. plural(years, "year") end
    if rest < 23328000 then return "over " .. plural(years, "year") end
    return "almost " .. plural(years + 1, "year")
  end

  function DurationM.humanize(d) return words(recv(d, Duration, "humanize")) end

  local now

  function DurationM.ago(d) return mktime(now() - recv(d, Duration, "ago")) end
  function DurationM.from_now(d) return mktime(now() + recv(d, Duration, "from_now")) end

  -- A Duration and a Duration; a Duration and a number (either side) for
  -- `*`; a Duration and a number or a Duration for `/`.
  local function both(sym, a, b)
    if not (isdur(a) and isdur(b)) then operands(sym, a, b) end
    return a[1], b[1]
  end
  local function isnum(v) return type(v) == "number" and v - v == 0 end

  function Duration.__add(a, b)
    local x, y = both("+", a, b)
    return mkdur(x + y)
  end
  function Duration.__sub(a, b)
    local x, y = both("-", a, b)
    return mkdur(x - y)
  end
  function Duration.__mul(a, b)
    if isdur(a) and isnum(b) then return mkdur(a[1] * b) end
    if isnum(a) and isdur(b) then return mkdur(a * b[1]) end
    operands("*", a, b)
  end
  function Duration.__div(a, b)
    if isdur(a) then
      if isdur(b) then
        if b[1] == 0 then I.throw("TypeError", "cannot divide a Duration by a zero Duration", 2) end
        return a[1] / b[1]
      end
      if isnum(b) then
        if b == 0 then I.throw("TypeError", "cannot divide a Duration by 0", 2) end
        return mkdur(a[1] / b)
      end
    end
    operands("/", a, b)
  end
  function Duration.__unm(a) return setmetatable({ -a[1] }, Duration) end
  function Duration.__eq(a, b) return a[1] == b[1] end
  function Duration.__lt(a, b)
    local x, y = both("<", a, b)
    return x < y
  end
  function Duration.__le(a, b)
    local x, y = both("<=", a, b)
    return x <= y
  end

  ---------------------------------------------------------------------------
  -- Time

  now = function()
    local e
    if host_now ~= nil then
      e = host_now()
    elseif os_time ~= nil then
      e = os_time()
    else
      I.throw("Exception", "Time.now needs a clock: the os library is not open here and the host gives no host.now", 3)
    end
    if type(e) ~= "number" or e - e ~= 0 then I.throw("Exception", "the clock gave " .. desc(e) .. ", not a finite Number", 3) end
    return e
  end

  -- `Time.now` reads like a value, as in Ruby: it is the one key of the
  -- library that runs on every read.
  setmetatable(Time, {
    __index = function(_, k)
      if k == "now" then return mktime(now()) end
      return nil
    end,
  })

  function Time.at(seconds)
    return mktime(num(seconds, "Time.at argument 1"))
  end

  -- The epoch of a local date and time: mktime's when there is one, else
  -- the UTC reading moved by the local offset, found twice so that an
  -- instant near a change of offset gets the offset in force there.
  local function local_epoch(y, mo, d, h, mi, s)
    local whole = floor(s)
    -- os.time's fields are C ints, which a field this large would wrap:
    -- such a date is found by the offset instead.
    if os_time ~= nil and abs(y) < 1e8 and abs(d) < 1e8 and abs(h) < 1e8 and abs(mi) < 1e8 and abs(whole) < 1e8 then
      local ok, e = pcall(os_time, { year = y, month = mo, day = d, hour = h, min = mi, sec = whole })
      if ok and type(e) == "number" then return e + (s - whole) end
    end
    local u = utc_epoch(y, mo, d, h, mi, whole)
    local e = u - offset_at(u)
    e = u - offset_at(e)
    return e + (s - whole)
  end

  -- `Time.new(year, month = 1, day = 1, hour = 0, min = 0, sec = 0,
  -- utc: false)`.
  function Time.new(...)
    local kw, n = split_kw(...)
    local y, mo, d, h, mi, s = ...
    if n < 6 then s = nil end
    if n < 5 then mi = nil end
    if n < 4 then h = nil end
    if n < 3 then d = nil end
    if n < 2 then mo = nil end
    int(y, "Time.new argument 1 (year)")
    mo = int(mo == nil and 1 or mo, "Time.new argument 2 (month)")
    d = int(d == nil and 1 or d, "Time.new argument 3 (day)")
    h = int(h == nil and 0 or h, "Time.new argument 4 (hour)")
    mi = int(mi == nil and 0 or mi, "Time.new argument 5 (min)")
    s = num(s == nil and 0 or s, "Time.new argument 6 (sec)")
    if utc_opt(kw, "Time.new") then return mktime(utc_epoch(y, mo, d, h, mi, s)) end
    -- An epoch out of range is so in local time too (the offset is small).
    mktime(utc_epoch(y, mo, d, h, mi, s))
    -- mktime takes a month in range; the years it carries are counted here.
    y = y + floor((mo - 1) / 12)
    mo = (mo - 1) % 12 + 1
    return mktime(local_epoch(y, mo, d, h, mi, s))
  end

  local function parse_error(s, why)
    I.throw("ConversionError", "cannot parse " .. sformat("%q", s) .. " as a Time: " .. why, 3)
  end

  -- `Time.parse(s, utc: false)`: ISO 8601's extended forms, `YYYY-MM-DD`,
  -- then optionally `T` (or `t`, or a space) and `HH:MM`, `:SS`, a
  -- fraction of a second after `.` or `,`, and a zone: `Z`, `±HH:MM`,
  -- `±HHMM` or `±HH`. Without a zone the time is local, or UTC with
  -- `utc: true`.
  function Time.parse(s, kw)
    if type(s) ~= "string" then bad("Time.parse argument 1", "String", s) end
    local utc = utc_opt(kw, "Time.parse") == true
    local y, mo, d, rest = smatch(s, "^(%d%d%d%d)%-(%d%d)%-(%d%d)(.*)$")
    if y == nil then parse_error(s, "expected YYYY-MM-DD") end
    y, mo, d = tonumber(y), tonumber(mo), tonumber(d)
    local h, mi, sec, frac = 0, 0, 0, 0
    if rest ~= "" then
      local hh, mm, more = smatch(rest, "^[Tt ](%d%d):(%d%d)(.*)$")
      if hh == nil then
        if smatch(rest, "^[Tt ]") then parse_error(s, "expected HH:MM after the date") end
      else
        h, mi, rest = tonumber(hh), tonumber(mm), more
        local ss, after = smatch(rest, "^:(%d%d)(.*)$")
        if ss ~= nil then
          sec, rest = tonumber(ss), after
          local digits, after2 = smatch(rest, "^[.,](%d+)(.*)$")
          if digits ~= nil then
            frac, rest = tonumber("0." .. digits), after2
          end
        end
      end
    end
    local off
    if rest == "Z" or rest == "z" then
      off = 0
    elseif rest ~= "" then
      local sign, oh, om = smatch(rest, "^([+-])(%d%d):?(%d%d)$")
      if sign == nil then
        sign, oh = smatch(rest, "^([+-])(%d%d)$")
        om = "00"
      end
      if sign == nil then parse_error(s, "unexpected " .. sformat("%q", rest)) end
      oh, om = tonumber(oh), tonumber(om)
      if oh > 23 or om > 59 then parse_error(s, "the zone offset is out of range") end
      off = (oh * 3600 + om * 60) * (sign == "-" and -1 or 1)
    end
    if mo < 1 or mo > 12 then parse_error(s, "the month is out of range") end
    if d < 1 or d > days_in_month(y, mo) then parse_error(s, "the day is out of range") end
    if h > 23 or mi > 59 or sec > 59 then parse_error(s, "the time of day is out of range") end
    if off == nil and not utc then return mktime(local_epoch(y, mo, d, h, mi, sec + frac)) end
    return mktime(utc_epoch(y, mo, d, h, mi, sec) + frac - (off or 0))
  end

  function TimeM.epoch(t) return recv(t, Time, "epoch") end

  -- The views (see the header): the same instant, read in UTC or local.
  function TimeM.utc(t)
    local e = recv(t, Time, "utc")
    if t[2] then return t end
    return mktime(e, true)
  end
  TimeM["local"] = function(t)
    local e = recv(t, Time, "local")
    if not t[2] then return t end
    return mktime(e, nil)
  end
  function TimeM.is_utc(t)
    recv(t, Time, "is_utc")
    return t[2] == true
  end

  -- The accessors: one field of the calendar each.
  do
    local function accessor(name, pick)
      TimeM[name] = function(t, kw)
        local e = recv(t, Time, name)
        local r = pick(fields(e, utc_of(t, kw, "Time." .. name)))
        return r
      end
    end
    accessor("year", function(y) return y end)
    accessor("month", function(_, mo) return mo end)
    accessor("day", function(_, _, d) return d end)
    accessor("hour", function(_, _, _, h) return h end)
    accessor("min", function(_, _, _, _, mi) return mi end)
    accessor("sec", function(_, _, _, _, _, s) return s end)
    accessor("wday", function(_, _, _, _, _, _, _, z) return (z + 4) % 7 end)
    accessor("yday", function(y, _, _, _, _, _, _, z) return z - days_from_civil(y, 1, 1) + 1 end)
  end

  function TimeM.utc_offset(t)
    local e = recv(t, Time, "utc_offset")
    if t[2] then return mkdur(0) end
    return mkdur(offset_at(floor(floor(e * 1000 + 0.5) / 1000)))
  end

  -- "+02:00", or "+0200" without the colon; "Z" for UTC when `z`.
  local function zone(off, utc, colon, z)
    if utc and z then return "Z" end
    local sign = off < 0 and "-" or "+"
    local a = floor(abs(off) / 60)
    return sformat(colon and "%s%02d:%02d" or "%s%02d%02d", sign, floor(a / 60), a % 60)
  end

  local wday_names = { "Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday" }
  local month_names = { "January", "February", "March", "April", "May", "June", "July", "August",
    "September", "October", "November", "December" }

  -- strftime with the directives of C and POSIX in the C locale (%a %A
  -- %b %B %c %C %d %D %e %F %h %H %I %j %k %l %m %M %n %p %r %R %s %S %t
  -- %T %u %w %x %X %y %Y %z %Z %%), and Ruby's %L (milliseconds) and %P
  -- (am/pm). Unspecified: %Z is "UTC" in UTC and the offset (+02:00) in
  -- local time, since Lua gives no portable name of the zone. A
  -- directive it does not know is kept as written.
  local directives
  local function format(fmt, e, utc)
    local y, mo, d, h, mi, s, ms, z, off = fields(e, utc)
    local wd = (z + 4) % 7
    local yd = z - days_from_civil(y, 1, 1) + 1
    local h12 = h % 12
    if h12 == 0 then h12 = 12 end
    local v = {
      Y = sformat("%04d", y), C = sformat("%02d", floor(y / 100)), y = sformat("%02d", y % 100),
      m = sformat("%02d", mo), d = sformat("%02d", d), e = sformat("%2d", d), j = sformat("%03d", yd),
      H = sformat("%02d", h), I = sformat("%02d", h12), k = sformat("%2d", h), l = sformat("%2d", h12),
      M = sformat("%02d", mi), S = sformat("%02d", s), L = sformat("%03d", ms),
      p = h < 12 and "AM" or "PM", P = h < 12 and "am" or "pm",
      a = ssub(wday_names[wd + 1], 1, 3), A = wday_names[wd + 1],
      b = ssub(month_names[mo], 1, 3), B = month_names[mo],
      u = tostring(wd == 0 and 7 or wd), w = tostring(wd),
      z = zone(off, utc, false, false), Z = utc and "UTC" or zone(off, false, true, false),
      s = sformat("%d", floor(floor(e * 1000 + 0.5) / 1000)), n = "\n", t = "\t", ["%"] = "%",
    }
    v.h = v.b
    return (sgsub(fmt, "%%(.)", function(c)
      local x = directives[c]
      if x ~= nil then return (sgsub(x, "%%(.)", v)) end
      return v[c]
    end))
  end
  -- The directives that are made of others.
  directives = {
    F = "%Y-%m-%d", T = "%H:%M:%S", D = "%m/%d/%y", R = "%H:%M", r = "%I:%M:%S %p",
    c = "%a %b %e %H:%M:%S %Y", x = "%m/%d/%y", X = "%H:%M:%S",
  }

  function TimeM.strftime(t, fmt, kw)
    local e = recv(t, Time, "strftime")
    if type(fmt) ~= "string" then bad("Time.strftime argument 1", "String", fmt) end
    return format(fmt, e, utc_of(t, kw, "Time.strftime"))
  end

  -- "2026-10-20T14:03:00Z" in UTC, "2026-10-20T16:03:00+02:00" in local
  -- time; ".250" after the seconds when there are milliseconds.
  local function iso(e, utc)
    local y, mo, d, h, mi, s, ms, _, off = fields(e, utc)
    return sformat("%04d-%02d-%02dT%02d:%02d:%02d", y, mo, d, h, mi, s)
      .. (ms ~= 0 and sformat(".%03d", ms) or "") .. zone(off, utc, true, true)
  end

  function TimeM.iso(t, kw) return iso(recv(t, Time, "iso"), utc_of(t, kw, "Time.iso")) end
  function TimeM.to_s(t) return iso(recv(t, Time, "to_s"), t[2] == true) end
  Time.__tostring = TimeM.to_s

  function TimeM.date(t, kw)
    local y, mo, d = fields(recv(t, Time, "date"), utc_of(t, kw, "Time.date"))
    return sformat("%04d-%02d-%02d", y, mo, d)
  end
  function TimeM.clock(t, kw)
    local _, _, _, h, mi, s = fields(recv(t, Time, "clock"), utc_of(t, kw, "Time.clock"))
    return sformat("%02d:%02d:%02d", h, mi, s)
  end

  function TimeM.beginning_of_day(t, kw)
    local e = recv(t, Time, "beginning_of_day")
    local utc = utc_of(t, kw, "Time.beginning_of_day")
    local y, mo, d = fields(e, utc)
    if utc then return mktime(utc_epoch(y, mo, d, 0, 0, 0), t[2]) end
    return mktime(local_epoch(y, mo, d, 0, 0, 0), t[2])
  end

  function TimeM.floor_to(t, step)
    local e = recv(t, Time, "floor_to")
    local s = check_dur(step, "Time.floor_to argument 1")
    if s <= 0 then I.throw("TypeError", "Time.floor_to argument 1 expects a positive Duration, got " .. tostring(step), 2) end
    return mktime(floor(e / s) * s, t[2])
  end

  function TimeM.since(t, earlier)
    local e = recv(t, Time, "since")
    if not istime(earlier) then bad("Time.since argument 1", "Time", earlier) end
    return mkdur(e - earlier[1])
  end

  TimeM["until"] = function(t, later)
    local e = recv(t, Time, "until")
    if not istime(later) then bad("Time.until argument 1", "Time", later) end
    return mkdur(later[1] - e)
  end

  local function reference(v, fname)
    if v == nil then return now() end
    if not istime(v) then bad(fname .. " argument 1", "Time", v) end
    return v[1]
  end

  -- "3 minutes ago", "in 2 hours", "just now" (under a second either
  -- way), relative to `now` (the clock's time when not given); the words
  -- are Duration's `humanize`.
  function TimeM.humanize(t, ref)
    local e = recv(t, Time, "humanize")
    local diff = e - reference(ref, "Time.humanize")
    if abs(diff) < 1 then return "just now" end
    if diff < 0 then return words(diff) .. " ago" end
    return "in " .. words(diff)
  end

  -- The distance alone, "3 minutes" (Rails' time_ago_in_words).
  function TimeM.ago_in_words(t, ref)
    local e = recv(t, Time, "ago_in_words")
    return words(e - reference(ref, "Time.ago_in_words"))
  end

  function Time.__add(a, b)
    if not (istime(a) and isdur(b)) then operands("+", a, b) end
    return mktime(a[1] + b[1], a[2])
  end
  function Time.__sub(a, b)
    if istime(a) then
      if istime(b) then return mkdur(a[1] - b[1]) end
      if isdur(b) then return mktime(a[1] - b[1], a[2]) end
    end
    operands("-", a, b)
  end
  function Time.__unm(a)
    I.throw("TypeError", "cannot apply '-' to " .. rt.typeof(a), 2)
  end
  function Time.__eq(a, b) return a[1] == b[1] end
  function Time.__lt(a, b)
    if not (istime(a) and istime(b)) then operands("<", a, b) end
    return a[1] < b[1]
  end
  function Time.__le(a, b)
    if not (istime(a) and istime(b)) then operands("<=", a, b) end
    return a[1] <= b[1]
  end

  for _, M in ipairs({ TimeM, DurationM }) do
    for _, f in pairs(M) do I.mark(f) end
  end

  -- Values cross to workers as tables with these metatables, and the
  -- types themselves by name (nl_64_worker.lua).
  rt.Worker.register("Time", Time)
  rt.Worker.register("Duration", Duration)
  rt.register("=nl runtime", "Time", Time)
  rt.register("=nl runtime", "Duration", Duration)
end
