-- Mixed workload: a small ray tracer. Vectors are tables with a class
-- metatable and methods called with `:`, spheres are tables in a list,
-- shading uses math.sqrt and math.max; one ray per pixel, one shadow ray
-- per hit.
local sqrt, max, floor = math.sqrt, math.max, math.floor
local scale = tonumber(os.getenv("NLB_SCALE") or "1")
local width = 480
local height = 360

local Vec3 = {}
Vec3.__index = Vec3

local function vec3(x, y, z)
  return setmetatable({ x = x, y = y, z = z }, Vec3)
end

function Vec3:add(o) return vec3(self.x + o.x, self.y + o.y, self.z + o.z) end
function Vec3:sub(o) return vec3(self.x - o.x, self.y - o.y, self.z - o.z) end
function Vec3:scale(k) return vec3(self.x * k, self.y * k, self.z * k) end
function Vec3:dot(o) return self.x * o.x + self.y * o.y + self.z * o.z end
function Vec3:unit()
  local len = sqrt(self.x * self.x + self.y * self.y + self.z * self.z)
  return vec3(self.x / len, self.y / len, self.z / len)
end

local Sphere = {}
Sphere.__index = Sphere

local function sphere(center, radius, shade)
  return setmetatable({ center = center, radius = radius, shade = shade }, Sphere)
end

-- The distance along the ray to the nearest hit of the sphere, or nil.
function Sphere:hit(origin, dir)
  local oc = origin:sub(self.center)
  local b = oc:dot(dir)
  local c = oc:dot(oc) - self.radius * self.radius
  local disc = b * b - c
  if disc < 0 then return nil end
  local t = -b - sqrt(disc)
  if t < 0.001 then return nil end
  return t
end

local spheres = {
  sphere(vec3(0, -1, 3), 1, 0.9),
  sphere(vec3(2, 0, 4), 1, 0.7),
  sphere(vec3(-2, 0, 4), 1, 0.5),
  sphere(vec3(0, -5001, 0), 5000, 0.3),
}
local light = vec3(1, 1, -1):unit()

local function nearest(origin, dir)
  local best, best_s = nil, nil
  for i = 1, #spheres do
    local s = spheres[i]
    local t = s:hit(origin, dir)
    if t ~= nil and (best == nil or t < best) then
      best, best_s = t, s
    end
  end
  return best, best_s
end

local function trace(origin, dir)
  local t, s = nearest(origin, dir)
  if t == nil then return 0.1 end
  local p = origin:add(dir:scale(t))
  local normal = p:sub(s.center):unit()
  local lit = max(0, normal:dot(light))
  local st = nearest(p, light)
  if st ~= nil then lit = lit * 0.3 end
  return s.shade * lit + 0.05
end

local check = 0
local eye = vec3(0, 0, -1)
for _ = 1, scale do
  for py = 0, height - 1 do
    for px = 0, width - 1 do
      local dir = vec3((px - width / 2) / height, (height / 2 - py) / height, 1):unit()
      check = check + floor(trace(eye, dir) * 255)
    end
  end
end
print(check)
