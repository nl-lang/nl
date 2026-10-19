-- Compares outputs of stock LuaJIT and of the fork, allowing exactly the
-- differences listed in expected-differences.txt.
--
--   luajit compare.lua suite  STOCK.out FORK.out EXPECTED RUNTIME
--       two runs of test/test.lua, compared test by test, by position (a
--       name can occur twice upstream); exit 0 when the only differences
--       are the suite entries for RUNTIME, each of them occurs, and each is
--       exactly as its SPEC says.
--   luajit compare.lua corpus FILE STOCK.out FORK.out EXPECTED RUNTIME
--       the outputs of one corpus file that differ; exit 0 when FILE has an
--       entry for RUNTIME and the difference is exactly as its SPEC says.
-- Prints what it found either way.

local mode = arg[1]

local function readlines(path)
  local t = {}
  for line in io.lines(path) do t[#t + 1] = line end
  return t
end

local function entries(path, kind, runtime)
  local t = {}
  for line in io.lines(path) do
    if not line:match("^#") and line:match("%S") then
      local k, rt, target, spec, why = line:match("^([^\t]+)\t([^\t]+)\t([^\t]+)\t([^\t]+)\t(.*)$")
      assert(k, "malformed line in " .. path .. ": " .. line)
      if k == kind and (rt == runtime or rt == "any") then
        t[target] = { spec = spec, why = why }
      end
    end
  end
  return t
end

local function set(list)
  local s = {}
  for item in list:gmatch("[^:]+") do s[item] = true end
  return s
end

-- The items added between `expected: "..."` and `got: "..."` in lines;
-- nil, message when items are missing or the lines are not there.
local function added_items(lines)
  local got, exp
  for _, l in ipairs(lines) do
    got = got or l:match('got: "([^"]*)"')
    exp = exp or l:match('^expected: "([^"]*)"')
  end
  if not got or not exp then return nil, "no got/expected lines" end
  local g, e = set(got), set(exp)
  for k in pairs(e) do
    if not g[k] then return nil, "missing from the fork: " .. k end
  end
  local add = {}
  for k in pairs(g) do if not e[k] then add[#add + 1] = k end end
  table.sort(add)
  return add
end

local function spec_items(spec)
  local t = {}
  for w in spec:gmatch("%S+") do t[#t + 1] = w end
  table.sort(t)
  return table.concat(t, " ")
end

local function blocks(lines)
  local order, cur = {}, nil
  for _, line in ipairs(lines) do
    local name = line:match("^%[%d+/%d+%] (.*)$")
    if name then
      cur = { name = name, text = {} }
      order[#order + 1] = cur
    elseif line:match("^%d+ passed") or line:match("^to run just failing")
           or line:match("^exit status ") then
      cur = nil
    elseif cur then
      cur.text[#cur.text + 1] = line
    end
  end
  return order
end

-- Compares two runs of test.lua; returns the number of problems.
local function compare_suite(slines, flines, exp)
  local so, fo = blocks(slines), blocks(flines)
  local bad, seen = 0, {}
  if #so ~= #fo then
    print("  different number of tests: stock " .. #so .. ", fork " .. #fo)
    return 1
  end
  for i, s in ipairs(so) do
    local f = fo[i]
    local st, ft = table.concat(s.text, "\n"), table.concat(f.text, "\n")
    if s.name ~= f.name then
      print("  test " .. i .. " is " .. s.name .. " on stock, " .. f.name .. " on the fork")
      bad = bad + 1
    elseif st ~= ft then
      local e = exp[s.name]
      local ok, why = false, "not listed"
      if e then
        local items = e.spec:match("^added:%s*(.*)$")
        if items and st == "" then
          local add, msg = added_items(f.text)
          if add and table.concat(add, " ") == spec_items(items) then ok = true
          else why = msg or ("added: " .. table.concat(add, " ") .. "; listed: " .. spec_items(items)) end
        elseif items then
          why = "stock does not pass it"
        elseif e.spec:match("^fork%-fails: ") then
          -- The fork fails a test that stock passes, with the given error.
          local pat = e.spec:match("^fork%-fails: (.*)$")
          if st == "" and (f.text[1] or ""):match(pat) then ok = true
          else why = "fork: " .. (f.text[1] or "(passed)") end
        else
          why = "unknown spec " .. e.spec
        end
      end
      if ok then
        seen[s.name] = true
        print("  expected: " .. s.name .. " (" .. e.spec .. ")")
      else
        bad = bad + 1
        print("  DIFFERENT: " .. s.name .. " [" .. why .. "]")
        print("    stock: " .. (st ~= "" and st:sub(1, 300) or "passed"))
        print("    fork:  " .. (ft ~= "" and ft:sub(1, 300) or "passed"))
      end
    end
  end
  for name in pairs(exp) do
    if not seen[name] then
      bad = bad + 1
      print("  expected difference did not occur: " .. name)
    end
  end
  return bad
end

if mode == "suite" then
  local exp = entries(arg[4], "suite", arg[5])
  os.exit(compare_suite(readlines(arg[2]), readlines(arg[3]), exp) == 0 and 0 or 1)
elseif mode == "corpus" then
  local file, slines, flines = arg[2], readlines(arg[3]), readlines(arg[4])
  local e = entries(arg[5], "corpus", arg[6])[file]
  if not e then print("not listed"); os.exit(1) end
  local spec = e.spec
  local items = spec:match("^first%-error%-added:%s*(.*)$")
  if items then
    local add, msg = added_items(flines)
    if add and table.concat(add, " ") == spec_items(items) then print(spec); os.exit(0) end
    print(msg or ("added: " .. table.concat(add, " ") .. "; listed: " .. spec_items(items)))
    os.exit(1)
  elseif spec == "suite" then
    os.exit(compare_suite(slines, flines, entries(arg[5], "suite", arg[6])) == 0 and 0 or 1)
  elseif spec:match("^extra%-lines: ") then
    local pat = spec:match("^extra%-lines: (.*)$")
    local rest, n = {}, 0
    for _, l in ipairs(flines) do
      if l:match(pat) then n = n + 1 else rest[#rest + 1] = l end
    end
    if n > 0 and table.concat(rest, "\n") == table.concat(slines, "\n") then print(spec); os.exit(0) end
    print("the fork's other lines differ, or no line matches " .. pat)
    os.exit(1)
  elseif spec:match("^fork%-fails: ") then
    local pat = spec:match("^fork%-fails: (.*)$")
    if (flines[1] or ""):match(pat) and flines[#flines] ~= "exit status 0" then print(spec); os.exit(0) end
    print("the fork does not fail as listed: " .. tostring(flines[1]))
    os.exit(1)
  end
  print("unknown spec " .. spec)
  os.exit(1)
end
error("usage: compare.lua suite|corpus ...")
