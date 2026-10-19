# Block parameters: `||`, names, annotations, defaults, a
# pattern, a trailing comma and a return type.
f do || end
f do |a, b,| end
f do |a: Number, b: String|: nil end
f do |a = 1| end
f do |{x, y}| end
f do |*rest| end
